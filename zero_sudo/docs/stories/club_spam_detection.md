<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: A Hacked Club Website Is Recognised From Its Own Sitemap, Without Fetching the Spam

As a **Platform Operator**, I want the club-website crawl to notice that a club's WordPress site has been filled with
casino, pharmacy and SEO spam, so that nothing is extracted from it and what the crawl already created from it can be taken
down, and I want that judgement to be a fixed set of rules I can read and test, not a model call. As a **Developer**, I want
one plain, Odoo-free module that both the crawl daemon (which collects the evidence) and Odoo (which decides) call, so the two
cannot drift apart.

The module is `zero_sudo/daemon/club_spam.py`. It imports nothing from `odoo`, and the stdlib is all it needs, so a standalone
systemd daemon loads it through a plain `sys.path` hop (the same arrangement as `robots_txt_policy.py`).

## The Problem

Bruce's instruction, 2026-10-22: "jasperares.org is full of spam. If a scan detects significant spam and the ETag is old enough
on a lot of it, drop the club." The county ARES site in question keeps its real pages (nets, frequencies, meetings) and also
lists several thousand casino pages in its sitemaps, its "recent posts" widget and two extra sitemaps that a spam plugin wrote
into `robots.txt`. Fetching thousands of spam pages to prove it would be rude and slow; the sitemaps already name them.

## Scoring a Page From Its Address `[@ANCHOR: zero_sudo:club_spam_score_text]`

`score_text(text)` scores a title or a slug already turned into words, and returns the points, the words that earned them, the
number of distinct amateur-radio words and whether the entry is flagged (3 points or more). The lexicon covers casino, slot,
betting, pharmacy, loans, essay mills, adult, crypto and forex, and SEO link selling, in English, Polish, Russian, Turkish,
Indonesian, Spanish, French, German, Italian, Portuguese, Dutch and Finnish, with compound words (`loistokasino`) caught by
their stem. Three families of words are weighed separately: strong (3, flagged alone), medium (2) and weak (1: `bonus`,
`deposit`, `odds`), plus phrases (`free spins`, `no deposit`, `slot gacor`). Three supporting signs add a point only when
something else already scored: a title in an unrelated language, a year stamp on a long slug (`...-2026`), and a repeat-post
suffix (`-2`, `-3`). Words that are ordinary at a radio club are removed first (`poker run`, `time slot`, `slot antenna`), and
two or more distinct amateur-radio words (ARES, hamfest, repeater, ...) take two points off, so a club's own "Casino Night"
page is not a spam page. A page in another language is never flagged for that alone.

`fold()` `[@ANCHOR: zero_sudo:club_spam_fold]` turns text into the form the lexicon matches (lower case, accents removed,
hyphens and apostrophes as spaces), `_tokens()` `[@ANCHOR: zero_sudo:club_spam_tokens]` splits it into words, and
`_non_latin_share()` `[@ANCHOR: zero_sudo:club_spam_non_latin_share]` measures how much of a title is in a non-Latin script.

`slug_of(url)` `[@ANCHOR: zero_sudo:club_spam_slug_of]` takes the words of an address's path and nothing of its host.
`score_entry(url, title)` `[@ANCHOR: zero_sudo:club_spam_score_entry]` scores a page from both and keeps the better, never
adding the two.

## Reading Sitemaps Without an XML Parser

A hostile site could send an entity bomb; a regular expression has no entities to expand, so sitemaps are read with regular expressions only.

### parse_sitemap `[@ANCHOR: zero_sudo:club_spam_parse_sitemap]`

**Signature.** `parse_sitemap(text, limit=MAX_SITEMAP_ENTRIES) -> (kind, entries, total)`

| Argument | Type | Meaning |
| --- | --- | --- |
| `text` | str | The sitemap file as fetched. Anything that is not a str is treated as empty. |
| `limit` | int | The most entries returned (default 5,000, `MAX_SITEMAP_ENTRIES`). |

**Returns.** `kind` is `"index"` when the text holds a `<sitemapindex>` (its `<sitemap>` blocks are read), `"urlset"` when it has `<url>` blocks or
a `<urlset>` tag, and `""` when neither. `entries` is a list of `(address, lastmod text)` pairs, in file order, up to `limit`: the `<loc>` is
read with or without CDATA, HTML-unescaped and cut to `MAX_URL_CHARS` (300); a block with no `<loc>` is skipped; the `<lastmod>` is the raw
text or `""`. `total` is the number of blocks in the file, which can exceed `len(entries)`, so a caller can tell a capped read.

**Errors.** None raised; bad input gives `("", [], 0)`. **Special.** Pure, no network, no state, safe from any thread. Cost is linear in the
text and bounded by the fetcher's size cap, since the regexes are non-nested. Tests: `zero_sudo/tests/test_club_spam.py` (real dates, an entity bomb).

### is_spam_feed_sitemap `[@ANCHOR: zero_sudo:club_spam_is_spam_feed_sitemap]`

`is_spam_feed_sitemap(url) -> bool` is True when `str(url or "")` contains `?feed=xmlsitemap` or `&feed=xmlsitemap`, with or without a number
after it (case-insensitive): the address shape the spam plugins inject into `robots.txt` (`/?feed=xmlsitemap1225`). No legitimate
WordPress sitemap plugin writes that (core uses `/wp-sitemap.xml`, Yoast `/sitemap_index.xml`). Any value, including `None`, is accepted;
nothing is raised. `build_evidence` counts the `robots.txt` sitemap addresses it matches (`robots_spam_sitemaps`) and `is_suspicious` treats any match as a signal. Pure, no network.

## How Old Is the Spam: Not From the ETag `[@ANCHOR: zero_sudo:club_spam_age_days]`

A WordPress page sends neither an `ETag` nor a `Last-Modified` header (checked live on the site above, spam pages and real ones
alike), so the server gives no validator to read. The age of a flagged address is taken from the sitemap's `<lastmod>`, which
WordPress core fills with the post's modified time. `age_days(text, today)` turns it into whole days and returns None for text
that is not a date, a year before 1995 (WordPress writes year -1 for an undated post) or a date in the future.
`dated_entries(entries, today)` `[@ANCHOR: zero_sudo:club_spam_dated_entries]` treats a sitemap whose dates are one day for nine
in ten of at least twenty URLs as STAMPED: the spam plugin's extra sitemaps carry the day the file was generated, which says
nothing about the pages, so their ages are None and cannot make the age gate fail. When too few flagged addresses have a usable
date, our own history decides: the time the site was first seen significantly spammed and not clean since.

## Reading a Home Page

A hide is recognised only in an inline `style` (`display:none`, `visibility:hidden`, a font size of 0 to 2 pixels, a large negative
indent or offset, zero opacity, `clip: rect(0`) or a `hidden` attribute: stylesheet classes are common in honest themes and are not read.

### extract_home_evidence `[@ANCHOR: zero_sudo:club_spam_extract_home_evidence]`

**Signature.** `extract_home_evidence(markup, base_url) -> dict`

| Argument | Type | Meaning |
| --- | --- | --- |
| `markup` | str | The home page HTML, cut to the first 2,500,000 characters; None or non-str reads as empty. |
| `base_url` | str | The address the page came from; used to make relative links absolute and to tell inside links from outbound ones (host compared without `www.`). |

**Returns.** A JSON-able dict: `wordpress` (True when the `generator` meta names WordPress or at least two `wp-content` / `api.w.org` markers
are present), `title` (whitespace-collapsed, 160 characters), `entries` (the page's own links, at most 300, each `{"u": absolute address
(300), "t": anchor text (160), "s": "recent" for the recent-posts widget, "hidden" for a link inside a hidden element, else "link", "x": True
when the host differs from the base}`, duplicates of the same address and text dropped), `link_count` (links the parser saw), `hidden_links`,
`hidden_chars` (characters of visible text inside hidden elements), `outbound_links` and `outbound_domains`. `mailto`, `tel`, `javascript`,
`sms`, `data` and bare `#` links are left out, as is a link to the page itself with only a fragment (the comment form's `#respond`).

**Errors.** None raised. A page that does not parse to the end (`ValueError` or `AssertionError` from the standard parser) is logged at info
level and what was read is used. **Special.** Pure, no network, no state; uses the standard library `html.parser`, which expands no entities or
document types, so a hostile page costs only its length.

### _HomeParser `[@ANCHOR: zero_sudo:club_spam_parser_init]`

`_HomeParser` is an `html.parser.HTMLParser` subclass with `convert_charrefs=True`. `__init__()` takes no arguments and sets the state the
three handlers share: `title` and `_in_title`, `generator` (the generator meta text, 80 characters), `wp_markers` (a counter), `links` (a
list of dicts `href, text, hidden, recent`), `hidden_chars`, `_stack` (a stack of `(tag, hidden, recent)` for the open elements, not a tree),
`_anchor` (the link being read) and `_skip_depth` (nesting inside script, style, noscript or template, whose text is ignored). One
instance serves one page and is not reusable or thread-safe.

### _HomeParser.handle_starttag `[@ANCHOR: zero_sudo:club_spam_parser_start]`

`handle_starttag(tag, attrs)` is called by the parser for each opening tag. It inherits the two flags from the parent on the stack:
`hidden` becomes true for an inline hiding style or a `hidden` attribute, and stays true for every descendant; `recent` becomes true when the
`id` or `class` matches a recent-posts widget name (`recent-posts`, `widget_recent_entries`, `latest-posts`). It records the WordPress
generator and marker attributes, sets `_in_title`, counts a skip-text element, pushes non-void elements on the stack, and for an `<a>` with an
`href` starts `_anchor` (address cut to 300 characters). Void elements (`br`, `img`, `meta`, ...) are never pushed. Returns nothing.

### _HomeParser.handle_endtag `[@ANCHOR: zero_sudo:club_spam_parser_end]`

`handle_endtag(tag)` pops the stack back to the nearest open element of that tag (a stray end tag with no match changes nothing, which
keeps broken markup from corrupting the flags), clears `_in_title` for `title`, and leaves a skip-text element. For `</a>` it finishes the
link: the text is whitespace-collapsed and cut to 160 characters, and the link is appended to `links` unless the page already has 1,200
links (`MAX_HOME_LINKS * 4`), which bounds memory on a link farm. Returns nothing.

### _HomeParser.handle_data `[@ANCHOR: zero_sudo:club_spam_parser_data]`

`handle_data(data)` receives text. It ignores text inside script, style, noscript and template; appends to the title while inside `<title>`;
appends to the open link's text; and adds the stripped length to `hidden_chars` when the innermost open element is hidden. Returns nothing.

### _join `[@ANCHOR: zero_sudo:club_spam_join]`

`_join(base_url, href) -> str` is `urllib.parse.urljoin(base_url, href)`: it makes a relative link absolute against the page address.
It exists as a named function so that both parsers resolve links the same way and tests can patch one place. `extract_home_evidence`
catches the `ValueError` that `urlsplit` raises for a malformed result; `_join` itself raises nothing for ordinary strings. Pure.

## The Evidence Sent to Odoo `[@ANCHOR: zero_sudo:club_spam_build_evidence]`

`build_evidence(home, sitemaps, robots_sitemaps, today, base_url)` puts everything in one compact, JSON-able dict: the home
page's links, the sitemap entries of the site's own host (spam-looking ones first and capped at 800, the whole at 1,100), the
total of URLs known, the number of flagged ones (so Odoo can believe a capped payload), and the `robots.txt` signal.
`is_suspicious(home, robots_sitemaps)` `[@ANCHOR: zero_sudo:club_spam_is_suspicious]` says whether a home page alone is bad
enough to be worth fetching the sitemaps (three flagged links on the page, or spam sitemaps announced by `robots.txt`).

## The Decision `[@ANCHOR: zero_sudo:club_spam_evaluate_evidence]`

`evaluate_evidence(evidence, config, first_detected_age_days)` re-scores every entry (the daemon is not believed) and returns
the level:

| Level | Meaning |
| --- | --- |
| `clean` | Almost nothing flagged. |
| `suspect` | At least 3 flagged URLs, or spam sitemaps in `robots.txt`, but not significant. |
| `significant` | At least `min_flagged` (10) distinct URLs AND at least `min_fraction` (30 percent) of the known URLs flagged. |
| `confirmed` | Significant AND persistent: of the flagged URLs with a usable date (at least `min_aged_known`, 10), at least `aged_fraction` (25 percent) are `age_days` (30) or more old; or, when too few are dated, the site has been significantly spammed since more than `age_days` ago by our own history. |

A daemon claim that more URLs are flagged than it sent is believed only when its sent slots are all full of entries this module
flags too. Anything that is not a well-formed evidence dict is clean, not an error.

`decide_recheck(verdict, days_since_last_detected, config)` `[@ANCHOR: zero_sudo:club_spam_decide_recheck]` says what to do with
a dropped site after a recheck: restore it when the scan is clean (under 5 percent flagged and under half the significance
count) and the spam was last seen `clean_days` (30) or more ago; keep it dropped otherwise.

## A Home Page That Redirects to a Spam Page

Found 2026-11-07: `w9vcl.com` answered with a redirect to a togel gambling page on an unrelated domain, and jasperares.org served the
same page before it went dark. The URL scan knew only three flagged URLs of 77, below the 10 URL / 30 percent bar, so the site stayed
"suspect". A club home page that leaves its registrable domain for a spam page is not the club's site, whatever its URL counts say.

Bruce's rule: a club site may change owners, so nothing about ownership, registration (RDAP, WHOIS) or hosting is checked. Only
the destination page is judged, with the lexicon that judges every other entry.

### `registrable_domain(value)` `[@ANCHOR: zero_sudo:club_spam_registrable_domain]`

| Item | Description |
| --- | --- |
| Purpose | The domain a site is registered under, to tell "another site" from "another page or name of the same site". |
| `value` | str: a host name or a URL, any case, with or without `www.`. None or "" gives "". |
| Returns | `w9vcl.com` for `https://www.W9VCL.com/x`; `example.co.uk` for `club.example.co.uk`. The standard library has no public-suffix list, so a two-letter country-code TLD preceded by one of `co com org net ac gov edu or ne go gob nom` keeps three labels and every other host two. An IP address or single label is returned unchanged. |
| Errors | None: a malformed value gives "". |
| Special | Pure, no network, no state, safe from any thread. A TLD outside the heuristic (for example a `.k12.wi.us` school) is treated as its last two labels, which can only make two sites look like one (a missed flag), never one site look like two. |

### `build_redirect(home_url, final_url, markup)` `[@ANCHOR: zero_sudo:club_spam_build_redirect]`

| Argument | Type | Meaning |
| --- | --- | --- |
| `home_url` | str | The club's website address, as stored. |
| `final_url` | str | The address the fetch ended on after every HTTP and meta-refresh redirect. |
| `markup` | str or None | The HTML of that final page (None or non-str: no title or text). |

Returns None when `final_url` is empty or has the same registrable domain as `home_url` (http to https, `www`, a subdomain: nothing
to report). Otherwise a dict, the redirect evidence, with every field bounded: `from` (the home address, 300 characters), `to`
(the final address, 300), `title` (160) and `text` (the first 400 characters of the page's visible text, scripts, styles and
`<head>` removed, entities decoded). Pure: makes no request and judges nothing. The dict is carried in the evidence as `"redirect"`
(see `build_evidence`) and is serialized as JSON, evidence version 1 (an older Odoo ignores the key).

### `_visible_text(markup)` `[@ANCHOR: zero_sudo:club_spam_visible_text]`

| Item | Description |
| --- | --- |
| Purpose | The page text `build_redirect` judges: the destination's HTML with the script, style, noscript and head elements and every other tag blanked. The destination is an untrusted site, so the cost must not depend on how hostile its markup is. |
| `markup` | str: the destination's HTML. Cut to `_MAX_MARKUP_CHARS` (2,500,000) characters first; the crawler's fetcher already caps a page at 2 MiB. |
| Returns | str: the markup with each `<script ...>...</script>` (also style, noscript, head; any case) replaced by one space, then each `<...>` replaced by one space. Entities are not decoded here (`build_redirect` does that). An element whose closing tag never comes stays in place and is stripped as an ordinary tag, and a `<` with no `>` after it stays as text. |
| Errors | None. |
| Special | Pure, no network, no state, safe from any thread. Linear in the length of the input: two regular expressions did this before and each rescanned to the end of the page from every unclosed `<script ` or bare `<`, taking 7 to 10 seconds on 100 kB of repeated `<script ` or `<head `. A closing tag that is absent once is absent for every later opening tag of that name, which is what keeps it linear. Tests: `test_65`, `test_66`. |

### `redirect_is_spam(redirect)` `[@ANCHOR: zero_sudo:club_spam_redirect_is_spam]`

`redirect` is the dict above, untrusted (it arrives from the daemon). True only when `from` and `to` are non-empty strings of
different registrable domains AND `score_entry(to, title)` is flagged, the lexicon and the ham-radio discount that
score every other entry, over the destination's address and title only (the page text is carried as evidence but not scored: body prose flagged an ordinary club page mentioning a casino night, and a non-English page with one lexicon word), so another club's page, or a club page about a "casino night", is not flagged. A redirect inside one
registrable domain is False even when its text is spam (that is the club's own page, and the URL scan covers it). A value that is
not a dict, or whose fields are missing or not strings, is False. Pure; raises nothing; no ownership lookup.

`evaluate_evidence()` uses it: a spam redirect makes the verdict `confirmed` at once (`persisted` True, `basis` "redirect", a
reason naming the destination, `redirect` True) and `head_sample` is the home address alone. There is no spam to age, so the 30 day gate does
not apply; the site is dropped in enforce mode (`would_drop` in dry run) on the first scan. A repaired site comes back through
the same HEAD re-check and recheck as any dropped site.

## Is the Spam Still There? `[@ANCHOR: zero_sudo:club_spam_pick_head_sample]`

`pick_head_sample(flagged, size)` chooses up to ten flagged addresses to re-check with HEAD: the oldest dated first, the rest
spread evenly. `evaluate_head_results(results, today, config=None, redirect=False)` `[@ANCHOR: zero_sudo:club_spam_evaluate_head_results]` judges the answers:
at least half 2xx is `still_spam`, more than half 404/410 (or a redirect to the home page) is `removed`, anything else
(403, 405, 5xx, a timeout) is `unknown` and is never counted as removed; it also counts the alive pages whose `Last-Modified` is
`age_days` or older (`http_date_age()` `[@ANCHOR: zero_sudo:club_spam_http_date_age]`) as persistence evidence.
With `redirect=True` (a site whose verdict basis is "redirect"; its sample is the home address) the question is the redirect, not the
page: a 3xx whose answer has `off_domain` true is `still_spam`; a 2xx (the home page answers itself again) or a 404/410 is
`removed`; a 3xx that stays on the domain, 403, 405, 5xx or a timeout is `unknown`, never `removed`. Each result may carry
`off_domain` (bool: the Location is on another registrable domain), set by the daemon's `PoliteFetcher.head`.

## Cutting Spam Out of a Page

### strip_spam_blocks `[@ANCHOR: zero_sudo:club_spam_strip_spam_blocks]`

**Purpose.** For a site that is suspect but not dropped: removes the spam from a page before a model reads it, so the model sees the club's own content.

**Signature.** `strip_spam_blocks(markup, base_url) -> (str, int)`

| Argument | Type | Meaning |
| --- | --- | --- |
| `markup` | str | The page HTML; None or non-str reads as empty. Not modified. |
| `base_url` | str | The page address, to make relative links absolute before scoring them. |

**Returns.** `(markup without its spam blocks, characters removed)`. Empty input gives `("", 0)`. Spans cut, by character offset in the
original text: a recent-posts widget at least half of whose links are flagged by `score_entry`; a hidden element that holds a flagged link;
a list item whose only link is flagged; and any other flagged link. Overlapping spans are merged. A page with nothing flagged comes back
unchanged with 0.

**Errors.** None raised: a parse failure (`ValueError`, `AssertionError`) is logged at info level and the spam found so far is cut.
**Special.** Pure, no network, no state. Uses the lexicon only, never a model.

### _StripParser `[@ANCHOR: zero_sudo:club_spam_strip_init]`

`_StripParser(source, base_url)` is an `HTMLParser` (`convert_charrefs=True`) that finds the spans. `source` is the full markup, kept to
locate tag ends, and `base_url` the page address. `__init__` stores both, builds `_line_starts` (the offset of each line start, so the
parser's `(line, column)` position can be turned into a character offset), creates the `_stack` of open-element dicts (`tag`, `start`,
`hidden`, `recent`, `anchors` as a list of flagged booleans, `href`, `text`) and the `spans` result list. Single use per page.

### _StripParser._offset `[@ANCHOR: zero_sudo:club_spam_strip_offset]`

`_offset() -> int` converts the parser's current position (`getpos()`, a 1-based line and 0-based column) into an offset into `source`,
using `_line_starts` (clamped to the last line). Called at every start tag, for the start of the span, and at every end tag, to find the
end of the closing tag with `source.find(">", ...)`. Pure; no errors.

### _StripParser.handle_starttag `[@ANCHOR: zero_sudo:club_spam_strip_start]`

`handle_starttag(tag, attrs)` pushes a frame for every non-void element recording where it starts, whether its own inline style hides it,
whether its `id` or `class` names a recent-posts widget, and for `<a>` its `href`. Unlike the home-page parser the flags are not
inherited: each frame stands alone because the rule is applied when the element closes. Void elements are skipped. Returns nothing.

### _StripParser.handle_data `[@ANCHOR: zero_sudo:club_spam_strip_data]`

`handle_data(data)` appends the text to every open `<a>` frame on the stack, so a link's anchor text is complete when its end tag comes
(and so nested markup inside a link counts). It does not look at any other text. Returns nothing.

### _StripParser.handle_endtag `[@ANCHOR: zero_sudo:club_spam_strip_end]`

`handle_endtag(tag)` closes the nearest open frame of that tag (an unmatched end tag is ignored) and decides. For an `<a>` with a real
`href` (not `mailto`, `tel`, `javascript`, `sms`, `data` or `#`) it makes the address absolute with `_join`, scores it with `score_entry`
together with the anchor text, records the flag in the frame and in the parent, and, if flagged and the parent is not a list item whose only
link this is, appends the link's span. For any other element it passes its link flags to the parent, then appends the element's span when
it is a recent-posts widget with at least half its links flagged, a hidden element with any flagged link, or a list item with exactly one link
and that one flagged. The span ends after the closing `>` found in `source`.

### urlopen_ssrf_safe with follow_redirects=False `[@ANCHOR: zero_sudo:ssrf_safe_fetch_no_redirect]`

`_NoRedirectHandler.redirect_request(req, fp, code, msg, headers, newurl)` in `zero_sudo/daemon/ssrf_safe_fetch.py` always returns `None`,
which is how `urllib` is told not to follow a redirect, so the 3xx answer reaches the caller as an `HTTPError` carrying the `Location`
header. `_build_ssrf_safe_opener(..., follow_redirects=False)` installs it in place of the stock `HTTPRedirectHandler`, and
`urlopen_ssrf_safe(..., follow_redirects=False)` selects it. The spam HEAD re-check uses this so that a HEAD stays on the host it was
asked about and a redirect to another registrable domain is reported (`off_domain`) and never fetched. The arguments are those of the
urllib handler (`req` the request, `fp` the response, `code` the status, `msg`, `headers`, `newurl` the target); the return value is always
`None`. No state, no I/O, no errors of its own. The SSRF validation of the address still happens before any connection.

## Settings `[@ANCHOR: zero_sudo:club_spam_config_from_params]`

`SpamConfig` holds every threshold with its default; `SpamConfig.from_params(get_param)` reads each from the system parameter of
the same name under `ham_repeater_dir.club_spam_` and keeps the default for a missing or unreadable value. `PARAM_NAMES` lists
them (and the `mode` parameter: `off`, `dry_run`, `enforce`) for the zero_sudo reader's whitelist. None of the names may contain
a word the reader treats as a secret.

## Verification

`zero_sudo/tests/test_club_spam.py` covers each rule over fixtures built to the real markup of a hacked WordPress club site
(in the same file): the spam titles and slugs, honest club titles that must not be flagged, sitemaps with
real dates, the stamped plugin sitemaps, the hidden link farm, an entity bomb, hostile and broken markup, a lying daemon, and the
persistence thresholds as a table.
