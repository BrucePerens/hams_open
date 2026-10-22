# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later

"""
Deterministic detection of a spam-compromised club website (docs/stories/club_spam_detection.md).

Bruce, 2026-10-22: "http://jasperares.org/ is full of spam. If a scan detects significant spam and the ETag is old
enough on a lot of it, drop the club." A WordPress site that was hacked (or whose comment/plugin hole was used) keeps its
real club pages and gains hundreds or thousands of casino, pharmacy and SEO pages that only the search engines are meant
to see. This module is the one copy of the rules that decide that, used by the crawl daemon (which collects the
evidence) and by Odoo (which re-scores it and decides). No model call, no network access, no `odoo` import (the same
decoupling rule as the rest of `zero_sudo/daemon/`): a standalone daemon reaches it through a plain `sys.path` hop.

Three layers, each a pure function over text:

1. SCORING. `score_entry(url, title)` scores one page, known only by its address and (when we have it) its title, from a
   multilingual lexicon of spam themes (casino, slot, betting, pharmacy, loans, essay mills, adult, crypto and forex
   scams, SEO link selling in English, Polish, Russian, Turkish, Indonesian, Spanish, French, German, Italian and
   Portuguese), keyword-stuffed slug patterns and a title in an unrelated language. An entry is FLAGGED at
   FLAG_POINTS (3) or more. Words that are ordinary in amateur radio ("poker run", "time slot") are removed first, and an
   entry with two or more distinct amateur-radio words has its points reduced, so a club's own "Casino Night" page is not
   a spam page.
2. EVIDENCE. `extract_home_evidence(html, url)` reads a home page with the standard library parser: the WordPress
   "recent posts" widget, every link, links hidden by inline styles, outbound links. `parse_sitemap(xml)` reads a sitemap
   index or URL set with regular expressions (never an XML parser: a hostile site could send an entity bomb).
   `build_evidence()` puts them in one compact JSON-able dict. A site with 200 spam slugs in its sitemap is detected from
   the sitemap alone: the spam pages themselves are never fetched.
3. DECISION. `evaluate_evidence(evidence, ...)` re-scores the entries (the daemon is not trusted to have done it right)
   and returns a verdict dict; `decide_recheck()` says whether a site that was dropped has been clean long enough to be
   restored. Table-driven in the tests from fixtures built to the real markup of jasperares.org.

WHY THE AGE GATE USES SITEMAP DATES. The instruction says "ETag is old enough". A WordPress page sends neither an ETag nor
a Last-Modified header (checked live on 2026-10-22 against the spam pages and the real ones of jasperares.org), so the
server gives no per-page validator at all. The age of each flagged URL is therefore taken, in this order, from: (1) the
sitemap's own `<lastmod>` (WordPress core writes the post's modified time there); (2) nothing, when the sitemap is
"stamped" (almost every URL carries the same date, which is what the spam plugin's injected `?feed=xmlsitemapNNNN`
sitemaps do: the date says when the file was generated, not when the page was written), and then (3) our own crawl
history: the time we first saw the site significantly spammed (`first_detected_age_days`). A stamped sitemap can never
make the age gate fail, only fail to help it.

Defaults of every threshold are in SpamConfig; the Odoo side reads them from system parameters with the same names.
"""

import html
import logging
import re
import unicodedata
from collections import Counter
from dataclasses import dataclass, fields as dataclass_fields
from datetime import date, datetime
from html.parser import HTMLParser
from urllib.parse import unquote, urljoin, urlsplit

logger = logging.getLogger(__name__)

FLAG_POINTS = 3
EVIDENCE_VERSION = 1
MAX_SITEMAP_ENTRIES = 5000
MAX_EVIDENCE_ENTRIES = 1100
MAX_FLAGGED_FIRST = 800
MAX_HOME_LINKS = 300
MAX_TITLE_CHARS = 160
MAX_URL_CHARS = 300
STAMPED_SITEMAP_MIN_ENTRIES = 20
STAMPED_SITEMAP_SHARE = 0.9

# Levels of a verdict, in increasing order of concern.
LEVEL_CLEAN = "clean"
LEVEL_SUSPECT = "suspect"
LEVEL_SIGNIFICANT = "significant"  # significant spam, not (yet) persistent
LEVEL_CONFIRMED = "confirmed"  # significant and persistent: the site is dropped
LEVELS = (LEVEL_CLEAN, LEVEL_SUSPECT, LEVEL_SIGNIFICANT, LEVEL_CONFIRMED)

# System parameter names (Odoo side), one per SpamConfig field. Registered with the zero_sudo reader's whitelist.
PARAM_PREFIX = "ham_repeater_dir.club_spam_"
MODE_OFF = "off"
MODE_DRY_RUN = "dry_run"
MODE_ENFORCE = "enforce"
MODES = (MODE_OFF, MODE_DRY_RUN, MODE_ENFORCE)

SITE_PROBLEM_KIND = "site_compromised_spam"


@dataclass(frozen=True)
class SpamConfig:
    """Every tunable, with its default. The Odoo system parameter for a field is PARAM_PREFIX + the field name."""

    # A site is significantly spammed when at least this many distinct URLs are flagged AND they are at least this share
    # of the URLs we know for the site. 10 keeps a handful of odd titles from ever mattering; 0.30 is Bruce's example.
    min_flagged: int = 10
    min_fraction: float = 0.30
    # Persistence: the spam has been there for age_days or more. Of the flagged URLs that have a usable date, at least
    # aged_fraction must be older than age_days (and at least min_aged_known of them must have a usable date) ...
    age_days: int = 30
    aged_fraction: float = 0.25
    min_aged_known: int = 10
    # ... or, when too few have a usable date, our own history says the site has been significantly spammed since more
    # than age_days ago (the first detection that has not been cleaned since).
    # A dropped site is looked at again after this many days, and restored when it has been clean at least clean_days
    # since the spam was last seen.
    recheck_days: int = 30
    clean_days: int = 30
    # A site is looked at (home page, then the sitemaps when it looks suspicious) at most this often.
    scan_days: int = 30
    # A site with at least this many flagged URLs (but not significant) is "suspect": its sitemaps are fetched.
    suspect_flagged: int = 3
    # A site that is suspect, significantly spammed or (dry run) would be dropped is looked at again this often, so
    # persistence is established without waiting a whole scan_days.
    rescan_flagged_days: int = 7

    # [@ANCHOR: zero_sudo:club_spam_config_from_params]
    @classmethod
    def from_params(cls, get_param):
        """Builds the config from `get_param(name) -> str|None`; a missing or unreadable value keeps its default."""
        values = {}
        for spec in dataclass_fields(cls):
            raw = get_param(PARAM_PREFIX + spec.name)
            if raw in (None, False, ""):
                continue
            try:
                values[spec.name] = spec.type(raw) if callable(spec.type) else raw
            except (TypeError, ValueError):
                continue
        return cls(**values)

PARAM_NAMES = tuple(PARAM_PREFIX + spec.name for spec in dataclass_fields(SpamConfig)) + (PARAM_PREFIX + "mode",)


# --------------------------------------------------------------------------------------------------------------------
# The lexicon. Everything is matched after fold(): lower case, accents removed, apostrophes and hyphens as spaces.

_STRONG = (
    "casino casinos casinon casinot casinoer kasino kasinot kasinon kasyno kasyna kasynie cassino cassinos cazinou "
    "gambling gambler gamblers betting wagering wetten judi togel gacor maxwin taruhan bahis bukmacher bukmacherskie bukmacherskich "
    "sportsbook bookmaker bookmakers scommesse apuestas apostas tragamonedas tragaperras pokies pokie blackjack "
    "roulette ruleta baccarat bakarat spielautomaten spielbank glucksspiel gokkasten gokken wettanbieter sportwetten "
    "freispiele hazardowe sbobet slots "
    "viagra cialis levitra tadalafil sildenafil kamagra "
    "escort escorts porn porno xxx onlyfans hentai "
    "payday paydayloan paydayloans counterfeit vuitton forex backlinks "
    "казино казиноо гемблинг букмекер букмекеры слоты "
    "casinoonline speelautomaten speelautomaat gokautomaten gokautomaat sloturi sloturile cazinouri pariuri ruletka "
    "ruletke ruletki blekdzhek kazino kazina automatech maszynach gamble gambles tragaperra sloty spielbanken "
    "scommessa betsoft"
).split()
_MEDIUM = (
    "slot spielothek apuesta wager wagers jackpot lottery lotto poker automaty kumar ставки "
    "pragmatic pkv loan loans essay essays dissertation replica replicas rolex pills cbd keto seo crypto cryptocurrency "
    "airdrop pharmacy craps giros hazardowych hracich keno"
).split()
_WEAK = (
    "bonus bonuses spins spin deposit gratis darmowe gry payout withdrawal coupon promo sambutan chip bandar daftar "
    "situs rtp odds bet bets paysafe paypal bitcoin dogecoin litecoin mastercard bingo cheap buy risultati partite pronostici supplement "
    "mortgage insurance"
).split()

_PHRASES = {
    "free spins": 3, "free spin": 3, "no deposit": 3, "deposit bonus": 3, "welcome bonus": 2, "bonus codes": 2,
    "slot gacor": 3, "slot online": 3, "slot machine": 3, "slot machines": 3, "situs slot": 3, "link alternatif": 3,
    "online casino": 3, "live casino": 3, "sports betting": 3, "sport betting": 3, "cash advance": 3,
    "payday loan": 3, "essay writing": 3, "write my essay": 3, "buy essay": 3, "homework help": 2,
    "replica watches": 3, "fake id": 3, "buy followers": 3, "seo services": 3, "link building": 3, "guest post": 2,
    "binary options": 3, "darmowe spiny": 3, "darmowe automaty": 3, "bez depozytu": 3, "gry hazardowe": 3,
    "automaty do gry": 3, "zaklady bukmacherskie": 3, "tanpa deposit": 3, "chip gratis": 3, "ohne einzahlung": 3,
    "senza deposito": 3, "giri gratis": 3, "giros gratis": 3, "sans depot": 3, "tours gratuits": 3,
    "machines a sous": 3, "paris sportifs": 3, "jeux d argent": 3, "juegos de azar": 3, "deneme bonusu": 3,
    "bonus veren": 3, "игровые автоматы": 3, "ставки на спорт": 3, "online pokies": 3, "real money": 1,
    "online pharmacy": 3, "pharmacy online": 3, "sic bo": 3, "sicbo": 3, "online slot": 3, "slot site": 3, "slot demo": 3, "demo free": 2, "bingo live": 3, "live bingo": 3, "weight loss": 2, "crypto trading": 3, "bitcoin casino": 3,
}
# Words and phrases that look like spam and are ordinary at a radio club: removed before scoring.
_BENIGN = re.compile(r"\b(?:poker runs?|time ?slots?|slot antennas?|slotted|card slot|expansion slot|bet you|gambleaware|be gamble aware)\b")

# A company that really offers these would not use them in an unrelated club's slug; a compound word (Finnish
# "loistokasino") is caught by the stem.
_COMPOUND_STEMS = ("casino", "kasino", "kasyno", "cassino", "cazino", "kazino", "gambl", "jackpot", "hazardow")

_HAM_TERMS = frozenset((
    "ares", "races", "arrl", "hamfest", "repeater", "repeaters", "amateur", "ham", "hams", "skywarn", "aprs",
    "emcomm", "callsign", "callsigns", "qso", "winlink", "dmr", "ysf", "dstar", "echolink", "allstar", "antenna",
    "antennas", "packet", "ncs", "licensing", "vec", "fcc", "dx", "qsl", "hf", "vhf", "uhf", "cw", "radios", "radio",
    "nets", "net", "frequencies", "frequency", "emergency", "preparedness",
))
_HAM_PHRASES = ("field day", "net control", "amateur radio", "ham radio", "net night")
# Only these count as the specific amateur-radio words that hold a score down (radio / net alone are too common).
_HAM_SPECIFIC = _HAM_TERMS - {"radio", "net", "nets", "frequency", "frequencies", "emergency", "preparedness", "dx", "cw",
                              "hf", "packet", "radios"}

_ENGLISH_WORDS = frozenset((
    "the and of to in for is on with at by from an how what best new our your you we are this that club meeting "
    "meetings news about contact events event members join information page home welcome county annual"
).split())
_FOREIGN_WORDS = frozenset((
    # Spanish, Portuguese
    "que los las una con para por del como mejor el la un es nao uma melhor os as da dos "
    # Italian
    "il che per nel della oggi sono dei delle migliori risultati partite di "
    # French
    "les des une pour dans avec sur est du sans meilleur nos "
    # German, Dutch
    "und der die das ist mit fur nicht auf ein eine het een voor "
    # Polish, Turkish, Indonesian
    "jak gdzie na nie roku graj najlepsze darmowe za od ve bir icin ile yeni dan yang untuk dengan tanpa terbaik cara ini"
).split())

_TOKEN = re.compile(r"[^\W_]+", re.UNICODE)
_YEAR_STAMP = re.compile(r"^20[2-3]\d$")
_COUNTER_SUFFIX = re.compile(r"^\d{1,2}$")


# [@ANCHOR: zero_sudo:club_spam_fold]
def fold(text):
    """Lower case, accents removed, apostrophes and hyphens as spaces: the form the lexicon is matched in."""
    text = unicodedata.normalize("NFKD", str(text or ""))
    text = "".join(ch for ch in text if not unicodedata.combining(ch)).lower()
    return text.replace("'", " ").replace("’", " ").replace("-", " ").replace("_", " ")


# [@ANCHOR: zero_sudo:club_spam_tokens]
def _tokens(folded):
    return _TOKEN.findall(folded)


# [@ANCHOR: zero_sudo:club_spam_non_latin_share]
def _non_latin_share(tokens):
    letters = [ch for tok in tokens for ch in tok if ch.isalpha()]
    if not letters:
        return 0.0
    return sum(1 for ch in letters if ord(ch) > 0x24F) / len(letters)


@dataclass
class EntryScore:
    points: int
    hits: list
    ham: int
    flagged: bool


# [@ANCHOR: zero_sudo:club_spam_score_text]
def score_text(text):
    """Scores a title or a slug already turned into words. Returns EntryScore(points, hits, ham, flagged)."""
    folded = _BENIGN.sub(" ", fold(text))
    tokens = _tokens(folded)
    if not tokens:
        return EntryScore(0, [], 0, False)
    hits, points = [], 0
    joined = " " + " ".join(tokens) + " "
    for phrase, weight in _PHRASES.items():
        if " " + phrase + " " in joined:
            hits.append(phrase)
            points += weight
    covered = {word for phrase in hits for word in phrase.split()}
    for tok in tokens:
        if tok in covered:
            continue
        weight = 3 if tok in _STRONG_SET else 2 if tok in _MEDIUM_SET else 1 if tok in _WEAK_SET else 0
        if not weight and len(tok) >= 8 and any(stem in tok for stem in _COMPOUND_STEMS):
            weight = 3
        if weight:
            hits.append(tok)
            points += weight
    ham = len({tok for tok in tokens if tok in _HAM_SPECIFIC}) + sum(1 for p in _HAM_PHRASES if " " + p + " " in joined)
    if points and ham >= 2:
        points = max(0, points - 2)
    if points >= 2 and not ham:
        foreign = sum(1 for tok in tokens if tok in _FOREIGN_WORDS)
        english = sum(1 for tok in tokens if tok in _ENGLISH_WORDS)
        if len(tokens) >= 3 and ((foreign >= 2 and english == 0) or _non_latin_share(tokens) >= 0.5):
            hits.append("foreign-language title")
            points += 1
        if len(tokens) >= 5 and _YEAR_STAMP.match(tokens[-1]):
            hits.append("year stamp")
            points += 1
        if len(tokens) >= 2 and _COUNTER_SUFFIX.match(tokens[-1]) and not _YEAR_STAMP.match(tokens[-1]):
            hits.append("repeat-post suffix")
            points += 1
    return EntryScore(points, hits, ham, points >= FLAG_POINTS)


_STRONG_SET = frozenset(_STRONG)
_MEDIUM_SET = frozenset(_MEDIUM)
_WEAK_SET = frozenset(_WEAK)


# [@ANCHOR: zero_sudo:club_spam_slug_of]
def slug_of(url):
    """The words of a URL's path (and nothing of its host), for scoring a page known only by its address. The query is
    ignored except that a `?feed=xmlsitemapNNNN` style address has no words at all."""
    try:
        path = unquote(urlsplit(str(url or "")).path)
    except ValueError:
        return ""
    path = re.sub(r"\.(?:html?|php|aspx?)$", "", path.strip("/"))
    return path.replace("/", " ")


# [@ANCHOR: zero_sudo:club_spam_score_entry]
def score_entry(url, title=""):
    """Scores one page from its address and, when known, its title. The better of the two counts (a hidden-link anchor
    text and a slug say the same thing in different words); the points are never added across them."""
    by_slug = score_text(slug_of(url))
    by_title = score_text(title) if title else EntryScore(0, [], 0, False)
    return by_title if by_title.points > by_slug.points else by_slug


# --------------------------------------------------------------------------------------------------------------------
# Sitemaps

_BLOCK_URL = re.compile(r"<url\b[^>]*>(.*?)</url>", re.I | re.S)
_BLOCK_SITEMAP = re.compile(r"<sitemap\b[^>]*>(.*?)</sitemap>", re.I | re.S)
_LOC = re.compile(r"<loc>\s*(?:<!\[CDATA\[)?\s*(.*?)\s*(?:\]\]>)?\s*</loc>", re.I | re.S)
_LASTMOD = re.compile(r"<lastmod>\s*(.*?)\s*</lastmod>", re.I | re.S)
_SPAM_FEED_SITEMAP = re.compile(r"[?&]feed=xmlsitemap\d*", re.I)


# [@ANCHOR: zero_sudo:club_spam_parse_sitemap]
def parse_sitemap(text, limit=MAX_SITEMAP_ENTRIES):
    """(kind, entries, total): kind "index" or "urlset" ("" when neither), entries [(loc, lastmod text)] up to `limit`,
    total the number of entries in the file. Regular expressions only."""
    text = text if isinstance(text, str) else ""
    index = bool(re.search(r"<sitemapindex\b", text, re.I))
    blocks = _BLOCK_SITEMAP.findall(text) if index else _BLOCK_URL.findall(text)
    kind = "index" if index else "urlset" if blocks or re.search(r"<urlset\b", text, re.I) else ""
    entries = []
    for block in blocks:
        loc = _LOC.search(block)
        if not loc:
            continue
        mod = _LASTMOD.search(block)
        if len(entries) < limit:
            entries.append((html.unescape(loc.group(1))[:MAX_URL_CHARS], mod.group(1) if mod else ""))
    return kind, entries, len(blocks)


# [@ANCHOR: zero_sudo:club_spam_is_spam_feed_sitemap]
def is_spam_feed_sitemap(url):
    """True for the address shape the spam plugins inject into robots.txt: `/?feed=xmlsitemap1225`. No legitimate
    WordPress sitemap plugin writes that (core uses /wp-sitemap.xml, Yoast /sitemap_index.xml)."""
    return bool(_SPAM_FEED_SITEMAP.search(str(url or "")))


# [@ANCHOR: zero_sudo:club_spam_age_days]
def age_days(lastmod, today):
    """Days between a sitemap date (`2026-09-01`, `2026-09-01T10:19:43-05:00`) and `today`; None when the text is not a
    date, is before 1995 (WordPress writes year -1 for a post with no date) or is in the future."""
    match = re.match(r"(\d{4})-(\d{2})-(\d{2})", str(lastmod or "").strip())
    if not match:
        return None
    try:
        when = date(int(match.group(1)), int(match.group(2)), int(match.group(3)))
    except ValueError:
        return None
    today = today.date() if isinstance(today, datetime) else today
    if when.year < 1995 or when > today:
        return None
    return (today - when).days


# [@ANCHOR: zero_sudo:club_spam_dated_entries]
def dated_entries(entries, today):
    """[(loc, age in days or None)] for a sitemap's [(loc, lastmod)]. A STAMPED sitemap (twenty or more URLs, nine in ten
    with one date) says when the file was generated, not when the pages were written, so every age is None."""
    ages = [age_days(mod, today) for _loc, mod in entries]
    known = [a for a in ages if a is not None]
    if len(entries) >= STAMPED_SITEMAP_MIN_ENTRIES and known:
        if Counter(known).most_common(1)[0][1] / len(entries) >= STAMPED_SITEMAP_SHARE:
            return [(loc, None) for loc, _mod in entries]
    return [(entries[i][0], ages[i]) for i in range(len(entries))]


# --------------------------------------------------------------------------------------------------------------------
# The home page

_HIDDEN_STYLE = re.compile(
    r"display\s*:\s*none|visibility\s*:\s*hidden|font-size\s*:\s*[012](?:px|pt|em|%)?\s*(?:;|$)|"
    r"text-indent\s*:\s*-\d{3,}|(?:left|top|right|bottom)\s*:\s*-\d{3,}|opacity\s*:\s*0(?:\.0+)?\s*(?:;|$)|"
    r"clip\s*:\s*rect\(\s*0",
    re.I,
)
_RECENT_WIDGET = re.compile(r"recent[-_ ]?(?:posts|entries)|widget_recent_entries|latest[-_ ]posts", re.I)
_VOID = frozenset("area base br col embed hr img input link meta param source track wbr".split())
_SKIP_TEXT = frozenset(("script", "style", "noscript", "template"))


class _HomeParser(HTMLParser):
    """Collects what the spam rules need from one home page. Not a tree: a stack of open tags with two inherited flags
    (hidden by an inline style, inside a recent-posts widget)."""

    # [@ANCHOR: zero_sudo:club_spam_parser_init]
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.title = ""
        self._in_title = False
        self.generator = ""
        self.wp_markers = 0
        self.links = []  # dicts: href, text, hidden, recent
        self.hidden_chars = 0
        self._stack = []  # (tag, hidden, recent)
        self._anchor = None
        self._skip_depth = 0

    # [@ANCHOR: zero_sudo:club_spam_parser_start]
    def handle_starttag(self, tag, attrs):
        attrs = {k: (v or "") for k, v in attrs}
        parent_hidden = self._stack[-1][1] if self._stack else False
        parent_recent = self._stack[-1][2] if self._stack else False
        if tag == "title":
            self._in_title = True
        if tag == "meta" and attrs.get("name", "").lower() == "generator":
            self.generator = attrs.get("content", "")[:80]
        if "wp-content" in attrs.get("href", "") + attrs.get("src", "") or "api.w.org" in attrs.get("rel", ""):
            self.wp_markers += 1
        hidden = parent_hidden or bool(_HIDDEN_STYLE.search(attrs.get("style", ""))) or "hidden" in attrs
        recent = parent_recent or bool(_RECENT_WIDGET.search(attrs.get("id", "") + " " + attrs.get("class", "")))
        if tag in _SKIP_TEXT:
            self._skip_depth += 1
        if tag in _VOID:
            return
        self._stack.append((tag, hidden, recent))
        if tag == "a" and attrs.get("href"):
            self._anchor = {"href": attrs["href"].strip()[:MAX_URL_CHARS], "text": "", "hidden": hidden, "recent": recent}

    # [@ANCHOR: zero_sudo:club_spam_parser_end]
    def handle_endtag(self, tag):
        if tag == "title":
            self._in_title = False
        if tag in _SKIP_TEXT and self._skip_depth:
            self._skip_depth -= 1
        for depth in range(len(self._stack) - 1, -1, -1):
            if self._stack[depth][0] == tag:
                del self._stack[depth:]
                break
        if tag == "a" and self._anchor is not None:
            self._anchor["text"] = " ".join(self._anchor["text"].split())[:MAX_TITLE_CHARS]
            if len(self.links) < MAX_HOME_LINKS * 4:
                self.links.append(self._anchor)
            self._anchor = None

    # [@ANCHOR: zero_sudo:club_spam_parser_data]
    def handle_data(self, data):
        if self._skip_depth:
            return
        if self._in_title:
            self.title += data
        if self._anchor is not None:
            self._anchor["text"] += data
        if self._stack and self._stack[-1][1]:
            self.hidden_chars += len(data.strip())


# [@ANCHOR: zero_sudo:club_spam_extract_home_evidence]
def extract_home_evidence(markup, base_url):
    """What a home page tells the spam rules: {"wordpress", "title", "entries", "link_count", "hidden_links",
    "hidden_chars", "outbound_links", "outbound_domains"}. `entries` are the page's own links (absolute address, anchor
    text, source "recent" for the recent-posts widget, "hidden" for a link inside an inline-styled hidden element,
    "link" otherwise); mailto, tel, javascript and in-page links are left out. CSS rules in a stylesheet are not read: a
    class that hides things is common in honest themes, and only an inline hide is the spammers' habit."""
    parser = _HomeParser()
    try:
        parser.feed(str(markup or "")[:2_500_000])
        parser.close()
    except (ValueError, AssertionError):  # a malformed page: use what was read
        logger.info("club_spam: the home page of %s did not parse to the end; the part read is used", base_url)
    base = urlsplit(base_url or "")
    base_host = (base.hostname or "").lower().removeprefix("www.")
    entries, seen = [], set()
    hidden_links = outbound_links = 0
    domains = set()
    for link in parser.links:
        href = link["href"]
        if re.match(r"(?i)(?:mailto|tel|javascript|sms|data):|#", href):
            continue
        try:
            absolute = urlsplit(href if re.match(r"(?i)https?://", href) else _join(base_url, href))
        except ValueError:
            continue
        if absolute.scheme not in ("http", "https") or not absolute.hostname:
            continue
        if absolute.fragment and absolute._replace(fragment="").geturl().rstrip("/") == str(base_url or "").rstrip("/"):
            continue  # an in-page anchor (the comment form's "#respond"): the page itself, not a link out
        host = absolute.hostname.lower().removeprefix("www.")
        external = host != base_host
        if link["hidden"]:
            hidden_links += 1
        if external:
            outbound_links += 1
            domains.add(host)
        key = (absolute.geturl(), link["text"])
        if key in seen or len(entries) >= MAX_HOME_LINKS:
            continue
        seen.add(key)
        source = "recent" if link["recent"] else "hidden" if link["hidden"] else "link"
        entries.append({"u": absolute.geturl()[:MAX_URL_CHARS], "t": link["text"], "s": source, "x": external})
    wordpress = "wordpress" in parser.generator.lower() or parser.wp_markers >= 2
    return {
        "wordpress": wordpress,
        "title": " ".join(parser.title.split())[:MAX_TITLE_CHARS],
        "entries": entries,
        "link_count": len(parser.links),
        "hidden_links": hidden_links,
        "hidden_chars": parser.hidden_chars,
        "outbound_links": outbound_links,
        "outbound_domains": len(domains),
    }


# [@ANCHOR: zero_sudo:club_spam_join]
def _join(base_url, href):
    return urljoin(base_url, href)


# --------------------------------------------------------------------------------------------------------------------
# Evidence: what the daemon sends to Odoo

# [@ANCHOR: zero_sudo:club_spam_build_evidence]
def build_evidence(home, sitemaps, robots_sitemaps, today, base_url=""):
    """One compact JSON-able dict from the home page's evidence (extract_home_evidence(), or None when the home page was
    not read), the sitemaps read ([{"url", "name", "kind", "entries": [(loc, lastmod)], "total"}]) and the sitemap
    addresses robots.txt lists. Spam-looking sitemap entries come first and are capped (MAX_FLAGGED_FIRST) so the payload
    stays small however large the infestation; `known` still counts every URL."""
    home = home or {}
    base_host = (urlsplit(base_url).hostname or "").lower().removeprefix("www.")
    flagged, other, known_total, sources = [], [], 0, {}
    for entry in home.get("entries", []):
        row = {"u": entry["u"], "t": entry.get("t", ""), "d": None, "s": entry.get("s", "link")}
        (flagged if score_entry(entry["u"], entry.get("t", "")).flagged else other).append(row)
    known_total += len(home.get("entries", []))
    for sitemap in sitemaps or []:
        entries = sitemap.get("entries", [])
        for loc, age in dated_entries(entries, today):
            if base_host and (urlsplit(loc).hostname or "").lower().removeprefix("www.") != base_host:
                continue
            row = {"u": loc, "t": "", "d": age, "s": sitemap.get("name", "sitemap")}
            (flagged if score_entry(loc).flagged else other).append(row)
        known_total += max(sitemap.get("total", 0), len(entries))
        sources[sitemap.get("name", "sitemap")] = sources.get(sitemap.get("name", "sitemap"), 0) + max(
            sitemap.get("total", 0), len(entries))
    flagged_total = len(flagged)
    flagged = flagged[:MAX_FLAGGED_FIRST]
    rows = (flagged + other)[:MAX_EVIDENCE_ENTRIES]
    return {
        "v": EVIDENCE_VERSION,
        "flagged_total": flagged_total,
        "wordpress": bool(home.get("wordpress")),
        "title": home.get("title", ""),
        "entries": rows,
        "known": max(known_total, len(rows)),
        "sources": sources,
        "robots_spam_sitemaps": sum(1 for url in (robots_sitemaps or []) if is_spam_feed_sitemap(url)),
        "hidden_links": int(home.get("hidden_links", 0)),
        "hidden_chars": int(home.get("hidden_chars", 0)),
        "outbound_links": int(home.get("outbound_links", 0)),
        "outbound_domains": int(home.get("outbound_domains", 0)),
        "home_read": bool(home),
    }


# --------------------------------------------------------------------------------------------------------------------
# The decision

# [@ANCHOR: zero_sudo:club_spam_is_suspicious]
def is_suspicious(home, robots_sitemaps, config=None):
    """True when a home page alone looks bad enough to be worth fetching the sitemaps: spam sitemaps announced by
    robots.txt, or `suspect_flagged` flagged links on the page (recent posts, hidden links, anything). Pure."""
    config = config or SpamConfig()
    if any(is_spam_feed_sitemap(url) for url in robots_sitemaps or []):
        return True
    flagged = sum(1 for e in (home or {}).get("entries", []) if score_entry(e["u"], e.get("t", "")).flagged)
    return flagged >= config.suspect_flagged


# [@ANCHOR: zero_sudo:club_spam_evaluate_evidence]
def evaluate_evidence(evidence, config=None, first_detected_age_days=None):
    """Re-scores the evidence and decides. `first_detected_age_days` is how long ago OUR history says this site first
    became significantly spammed without being clean since (None when never). Returns a verdict dict:

    level (clean | suspect | significant | confirmed), flagged, known, fraction, aged_flagged, dated_flagged,
    on_topic, persisted, basis ("lastmod", "history" or ""), reasons (list of text), sample (up to 15 flagged
    {"url", "title", "points", "hits", "age"})."""
    config = config or SpamConfig()
    rows = (evidence or {}).get("entries", []) if isinstance(evidence, dict) else []
    best = {}
    for row in rows if isinstance(rows, list) else []:
        if not isinstance(row, dict) or not isinstance(row.get("u"), str):
            continue
        key = row["u"].lower().rstrip("/")
        score = score_entry(row["u"], str(row.get("t") or "")[:MAX_TITLE_CHARS])
        age = row.get("d") if isinstance(row.get("d"), int) and not isinstance(row.get("d"), bool) and row["d"] >= 0 else None
        old = best.get(key)
        if old is None or (age is not None and old["age"] is None) or score.points > old["score"].points:
            best[key] = {"row": row, "score": score, "age": age if age is not None else (old or {}).get("age")}
    flagged = [item for item in best.values() if item["score"].flagged]
    sent_known = len(best)
    declared = evidence.get("known") if isinstance(evidence, dict) and isinstance(evidence.get("known"), int) else 0
    known = max(sent_known, min(declared, 10_000_000))
    n_flagged = len(flagged)
    declared_flagged = evidence.get("flagged_total") if isinstance(evidence, dict) else 0
    # The daemon sends at most MAX_FLAGGED_FIRST flagged entries. When that cap was reached (every slot holds an entry
    # this scorer flags too) the true count is larger and the daemon's own total, which cannot exceed what is known, is
    # believed; otherwise only what was re-scored here counts.
    if (
        isinstance(declared_flagged, int) and declared_flagged > n_flagged
        and n_flagged >= 0.95 * MAX_FLAGGED_FIRST
    ):
        n_flagged = min(declared_flagged, known)
    fraction = n_flagged / known if known else 0.0
    on_topic = sum(
        1 for item in best.values()
        if not item["score"].flagged and item["score"].ham >= 1
    )
    dated = [item for item in flagged if item["age"] is not None]
    aged = [item for item in dated if item["age"] >= config.age_days]
    reasons = []
    feeds = int((evidence or {}).get("robots_spam_sitemaps", 0) or 0) if isinstance(evidence, dict) else 0
    if feeds:
        reasons.append(f"robots.txt announces {feeds} spam-plugin sitemap(s) (?feed=xmlsitemapNNNN)")
    significant = n_flagged >= config.min_flagged and fraction >= config.min_fraction
    persisted, basis = False, ""
    if significant:
        reasons.append(f"{n_flagged} of {known} known URLs ({fraction:.0%}) look like spam")
        if len(dated) >= config.min_aged_known:
            share = len(aged) / len(dated)
            if share >= config.aged_fraction:
                persisted, basis = True, "lastmod"
                reasons.append(
                    f"{len(aged)} of {len(dated)} dated spam URLs ({share:.0%}) are {config.age_days}+ days old"
                )
            else:
                reasons.append(
                    f"only {len(aged)} of {len(dated)} dated spam URLs ({share:.0%}) are {config.age_days}+ days old"
                )
        if not persisted:
            if first_detected_age_days is not None and first_detected_age_days >= config.age_days:
                persisted, basis = True, "history"
                reasons.append(f"first seen significantly spammed {int(first_detected_age_days)} days ago")
            else:
                reasons.append("not yet persistent: too few dated spam URLs are old and the history is younger")
        level = LEVEL_CONFIRMED if persisted else LEVEL_SIGNIFICANT
    elif n_flagged >= config.suspect_flagged or feeds:
        level = LEVEL_SUSPECT
        reasons.append(f"{n_flagged} of {known} known URLs flagged: below the {config.min_flagged}/{config.min_fraction:.0%} bar")
    else:
        level = LEVEL_CLEAN
    flagged.sort(key=lambda item: (-item["score"].points, item["row"]["u"]))
    sample = [
        {
            "url": item["row"]["u"][:MAX_URL_CHARS], "title": str(item["row"].get("t") or "")[:MAX_TITLE_CHARS],
            "points": item["score"].points, "hits": item["score"].hits[:6], "age": item["age"],
        }
        for item in flagged[:15]
    ]
    return {
        "level": level, "flagged": n_flagged, "known": known, "fraction": round(fraction, 4),
        "aged_flagged": len(aged), "dated_flagged": len(dated), "on_topic": on_topic,
        "persisted": persisted, "basis": basis, "reasons": reasons, "sample": sample,
        "wordpress": bool(evidence.get("wordpress")) if isinstance(evidence, dict) else False,
    }


# [@ANCHOR: zero_sudo:club_spam_decide_recheck]
def decide_recheck(verdict, days_since_last_detected, config=None):
    """What to do with a dropped site after a recheck: "restore" when the scan finds it clean (nothing significant, under
    5 percent flagged and under half the significance count) and the spam was last seen clean_days or more ago, "keep"
    otherwise (including a clean scan too soon after the last sighting, which waits for the next recheck)."""
    config = config or SpamConfig()
    clean = (
        verdict.get("level") in (LEVEL_CLEAN, LEVEL_SUSPECT)
        and verdict.get("fraction", 1) < 0.05
        and verdict.get("flagged", 0) < max(1, config.min_flagged // 2)
    )
    if clean and days_since_last_detected is not None and days_since_last_detected >= config.clean_days:
        return "restore"
    return "keep"
