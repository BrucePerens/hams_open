# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""
Tests of `zero_sudo/daemon/club_spam.py`, the pure rules that decide a club website is spam-compromised
(docs/stories/club_spam_detection.md). Table-driven, over fixtures built to the real markup of a hacked WordPress club site
(the fixtures are at the top of this file). Plain `unittest`: the module is loaded by path, so the file runs the same under
Odoo's test runner and under pytest, with no database and no network.
"""
import importlib.util
import os
import sys
import unittest

_HERE = os.path.dirname(os.path.abspath(__file__))


def _load(name, *parts):
    spec = importlib.util.spec_from_file_location(name, os.path.join(_HERE, *parts))
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


spam = _load("club_spam_under_test", "..", "daemon", "club_spam.py")

# ---- Fixtures: synthetic, built to the real markup of a hacked WordPress club site ----------------------------
# Synthetic fixtures for tests of zero_sudo/daemon/club_spam.py, built to the real markup of a hacked WordPress club site
# (observed 2026-10-22 on a county ARES club's site; no text, club, person or address of that site is copied except the spam
# slugs and titles, which are public spam and are what the tests are about). Plain Python, no Odoo.
#
# * `home_html()`: a WordPress home page with the site's menu, a "recent posts" widget (the markup core WordPress writes), a
#   comment form whose Cancel-reply link carries an inline `display:none` (honest, and must not count), and optional extras;
# * `sitemap_index()`, `urlset()`: core WordPress sitemaps (`<lastmod>` is the post's modified time, with the year -0001
#   WordPress writes for a post with no date), and the spam plugin's `?feed=xmlsitemapNNNN` file in which every URL carries
#   the day the file was generated;
# * `ROBOTS_*`: a clean robots.txt and the one the spam plugin edited.
import datetime

HOST = "http://club.example.org"
TODAY = datetime.date(2026, 10, 22)

SPAM_RECENT_POSTS = [
    ("risultati-partite-di-hockey-di-oggi-serie-a", "Risultati Partite Di Hockey Di Oggi Serie A", "October 15, 2026"),
    ("que-es-un-calibre-digital-de-blackjack", "Qué Es Un Calibre Digital De Blackjack", "October 12, 2026"),
    ("risultati-partite-di-hockey-di-oggi-serie-a-2", "Risultati Partite Di Hockey Di Oggi Serie A", "October 11, 2026"),
    ("darmowe-automaty-3-bebnowe-gdzie-grac-w-2026-roku", "Darmowe Automaty 3 Bębnowe Gdzie Grać W 2026 Roku",
     "October 10, 2026"),
    ("bordeaux-casino-no-deposit-bonus-codes-for-free-spins-2026",
     "Bordeaux Casino No Deposit Bonus Codes For Free Spins 2026", "October 9, 2026"),
]
CLEAN_RECENT_POSTS = [
    ("october-ares-meeting", "October ARES Meeting", "October 2, 2026"),
    ("weekly-net", "Weekly Net", "September 28, 2026"),
    ("field-day-2026-results", "Field Day 2026 Results", "July 1, 2026"),
]
# Slugs the spam plugin injected into the extra sitemaps, as observed.
INJECTED_SLUGS = [
    "sic-bo-paysafe-welcome-bonus-australia", "betexpress-casino-no-deposit-bonus-au-2026",
    "slot-no-deposit-bonus-codes-australia", "wildrobin-casino-free-spins-au-2026", "online-poker-earn-real-money",
    "best-online-rummy-no-wagering-casino-australia", "5-dollar-free-slot-bonus-no-deposit",
    "make-money-with-online-gambling-australia", "100-no-deposit-spins-australia", "fun-online-casino",
    "ignition-casino-15-free-spins-no-deposit-au", "best-revolut-casino-safe-casino-australia",
    "blackjack-online-players", "american-roulette-numbers", "free-live-casino-roulette", "playing-slots-to-win",
    "free-casino-bonus-no-wagering", "fitzdares-casino-no-deposit-free-spins-uk-new-customers-2026",
    "unregulated-slots-no-deposit-uk", "blackjack-online-6-deck-free", "best-casino-games-uk",
    "dollar-1-min-deposit-casino-uk", "online-slots-prize-draw-casino-uk", "120-free-spins-promotion-real-in-uk",
]
# Spam posts WordPress itself lists in its core sitemap, with the dates it wrote.
CORE_SPAM = [
    ("online-gambling-real-money-app", "2026-07-26"), ("superlines-casino-login", "2026-03-04"),
    ("play-for-real-cash-slots-uk", "2025-11-19"), ("best-site-for-online-gambling", "2025-11-10"),
    ("virtuele-gokautomaten-verzekering-hoe-het-werkt", "2026-06-22"), ("spielbanken-europa-auszahlung", "2025-11-12"),
    ("como-puedo-retirar-dinero-de-las-tragaperras-online-en-el-2026", "2026-01-01"),
    ("quelle-est-la-meilleure-methode-pour-gagner-a-la-machine-de-roulette", "2025-10-21"),
    ("tonybet-casino-100-free-spins-bonus-2026", "2025-10-24"), ("je-blackjack-len-hra-v-roku-2026", "2026-02-09"),
    ("online-kasino-penize-zpet-2026", "2025-12-06"), ("real-cash-mobile-slots-uk", "2026-04-29"),
    ("sportsbet-casino-no-deposit-bonus-100-free-spins", "2026-07-08"),
    ("win-real-money-online-instantly-australia-no-deposit", "2026-02-18"),
    ("online-casino-roulette-mit-hohen-gewinnen", "2025-12-17"),
]
# The club's own pages and posts, with real-looking dates.
CORE_CLEAN = [
    ("weekly-net", "2014-11-10"), ("webb-city-parade-2020", "2020-12-02"), ("january-ares-meeting", "2020-12-02"),
    ("november-ares-meeting", "2020-12-02"), ("february-2021-net-controls", "2021-02-03"),
    ("jasper-county-march-2021-net-controls", "2021-03-02"), ("august-3-2021-ares-meeting", "2021-07-26"),
    ("ares-flyer", "2021-09-08"), ("joplin-hamfest-september-30-thru-october-1-2022", "2022-08-23"),
    ("jasper-county-ares-drill-08-16-22", "2022-08-16"), ("ares-attendance", "2023-02-10"),
    ("ares-christmas-donation-receiver-loving-grace", "2023-11-13"), ("spiva-park-decorating", "2022-11-17"),
    ("frequencies", "2016-03-08"), ("calendar", "2016-03-08"), ("contact", "2014-11-10"), ("volunteer", "2014-11-10"),
    ("about", "2014-11-10"), ("links", "2014-11-10"), ("weather", "2022-03-09"),
]


def home_html(recent=CLEAN_RECENT_POSTS, body_extra="", head_extra=""):
    """A WordPress home page. `recent` is [(slug, title, date text)]."""
    items = "\n".join(
        f'\t<li>\n\t\t<a href="{HOST}/{slug}/">{title}</a>\n\t\t<span class="post-date">{when}</span>\n\t</li>'
        for slug, title, when in recent
    )
    return f"""<!DOCTYPE html>
<html lang="en-US">
<head>
<meta charset="UTF-8">
<title>Example County ARES &#8211; Amateur Radio Emergency Services of Example County</title>
<meta name="generator" content="WordPress 6.8.1" />
<link rel="https://api.w.org/" href="{HOST}/wp-json/" />
<link rel='stylesheet' id='decode-style-css' href='{HOST}/wp-content/themes/decode/style.css' type='text/css' />
{head_extra}
</head>
<body class="home page-template-default page">
<a class="skip-link screen-reader-text" href="#content">Skip to content</a>
<nav id="site-navigation"><ul id="menu-main">
<li><a href="{HOST}/">Home</a></li><li><a href="{HOST}/news/">News</a></li><li><a href="{HOST}/about/">About</a></li>
<li><a href="{HOST}/calendar/">Calendar</a></li><li><a href="{HOST}/frequencies/">Frequencies / Nets</a></li>
<li><a href="{HOST}/contact/">Contact</a></li><li><a href="{HOST}/links/">Links</a></li>
</ul></nav>
<main id="content"><article><h1>Welcome</h1>
<p>We are the Amateur Radio Emergency Service of Example County. Weekly net: Tuesday 7 pm on the 146.940 repeater.</p>
<p><a href="http://www.arrl.org/ares">ARRL ARES</a> | <a href="https://www.weather.gov/wrn/">NWS Weather Ready Nation</a></p>
</article>
<div id="respond" class="comment-respond">
<h3 id="reply-title" class="comment-reply-title">Leave a Reply <small><a rel="nofollow" id="cancel-comment-reply-link" href="/#respond" style="display:none;">Cancel reply</a></small></h3>
<p class="must-log-in">You must be <a href="{HOST}/wp-login.php?redirect_to=x">logged in</a> to post a comment.</p>
</div></main>
<aside id="recent-posts-2" class="widget widget_recent_entries">
<h2 class="widgettitle">News</h2>
<ul>
{items}
</ul>
</aside>
{body_extra}
<footer><a href="https://www.machothemes.com/themes/decode/">Decode Theme</a></footer>
</body></html>
"""


def hidden_link_block(count=25, style="position:absolute;left:-9999px;"):
    """The block a spam injector hides in a page: links with casino anchor text inside an inline-styled hidden div."""
    links = "".join(
        f'<a href="http://casino-{i}.example.net/free-spins-no-deposit">Best online casino {i} free spins</a> '
        for i in range(count)
    )
    return f'<div style="{style}">{links}</div>'


def sitemap_index(children):
    body = "".join(f"<sitemap><loc>{HOST}/{name}</loc></sitemap>" for name in children)
    return (
        '<?xml version="1.0" encoding="UTF-8"?>\n<?xml-stylesheet type="text/xsl" href="x.xsl" ?>\n'
        f'<sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">{body}</sitemapindex>'
    )


def urlset(entries, indent=False):
    """entries [(slug or full url, lastmod text or '')]. `indent` writes the spam plugin's spaced layout."""
    parts = []
    for slug, mod in entries:
        loc = slug if slug.startswith("http") else f"{HOST}/{slug}/"
        if indent:
            parts.append(f"   <url>\n      <loc>{loc}</loc>\n      <lastmod>{mod}</lastmod>\n   </url>\n")
        else:
            parts.append(f"<url><loc>{loc}</loc>" + (f"<lastmod>{mod}</lastmod>" if mod else "") + "</url>")
    return (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        f'<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">{"".join(parts)}</urlset>'
    )


def core_posts_sitemap(spam=CORE_SPAM, clean=CORE_CLEAN, spam_times=1):
    """A core posts sitemap with `spam_times` copies of the spam list (suffixed -2, -3 ... like the mass posters)."""
    entries = [(slug, f"{day}T10:19:43-05:00") for slug, day in clean]
    for n in range(spam_times):
        for slug, day in spam:
            entries.append((slug if n == 0 else f"{slug}-{n + 1}", f"{day}T10:19:43-05:00"))
    return urlset(entries)


def injected_sitemap(times=3, stamp="2026-10-18"):
    """The spam plugin's extra sitemap: the slugs, repeated with numeric tags, every one stamped with the same day."""
    entries = []
    for n in range(times):
        for slug in INJECTED_SLUGS:
            entries.append((slug if n == 0 else f"{slug}-{n + 1}", stamp))
    return urlset(entries, indent=True)


ROBOTS_CLEAN = "User-agent: *\nDisallow: /wp-admin/\nAllow: /wp-admin/admin-ajax.php\n"
ROBOTS_SPAM = (
    ROBOTS_CLEAN + f"Sitemap: {HOST}/?feed=xmlsitemap1225\nSitemap: {HOST}/?feed=xmlsitemap1301\n"
)




def _evidence(home_markup=None, sitemaps=(), robots=(), today=TODAY):
    home = spam.extract_home_evidence(home_markup, HOST + "/") if home_markup is not None else None
    return spam.build_evidence(home, list(sitemaps), list(robots), today, HOST + "/")


def _sitemap(name, xml):
    kind, entries, total = spam.parse_sitemap(xml)
    return {"name": name, "url": name, "entries": entries, "total": total, "kind": kind}


class TestScoring(unittest.TestCase):
    """The lexicon: what is flagged, and what an amateur radio club legitimately writes is not."""

    def test_01_the_spam_a_hacked_club_site_publishes_is_flagged(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_score_text]
        # Tests [@ANCHOR: zero_sudo:club_spam_fold]
        # Tests [@ANCHOR: zero_sudo:club_spam_tokens]
        # Tests [@ANCHOR: zero_sudo:club_spam_non_latin_share]
        titles = [title for _slug, title, _when in SPAM_RECENT_POSTS] + [
            "Best Online Casino In Canada 2026", "Buy Cialis Online No Prescription", "Slot Gacor Hari Ini Maxwin",
            "Лучшее онлайн казино с бонусом", "Cele Mai Sigure Cazinouri Online Cu Sloturi Fara Depozit",
            "Payday Loans Same Day Cash Advance", "Write My Essay Cheap Essay Writing Service",
            "Mobiel Speelautomaten Met Instant Bonus", "Wo Kann Man Spielbanken Gratis Spielen",
            "Escort Girls Porn Xxx", "Free Spins No Deposit Bonus Codes",
        ]
        for title in titles:
            with self.subTest(title=title):
                self.assertTrue(spam.score_text(title).flagged, spam.score_text(title))

    def test_02_what_a_radio_club_really_writes_is_not_flagged(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_score_text]
        honest = [
            "Weekly Net", "January ARES Meeting", "Field Day 2026 Poker Run", "Time slots for the Tuesday net",
            "Casino Night Fundraiser for the ARES hamfest", "Contest Bonus Points Rules", "Deposit Dues By Check",
            "Slot antenna design for 2 meters", "Hamfest Raffle And Bingo Night", "Repeater Coverage Map",
            "Winlink Gateway Packet Setup", "Reunión mensual del club de radioaficionados", "Cena de fin de año del radio club",
            "Treffen des Amateurfunk Ortsverbands", "Antenna Hazard Safety Training", "Drug Take Back Day Volunteer Net Control",
            "Jasper County ARES Net Controls August 2021", "Jasper County ARES Bet Controls For August 2022", "Swap Meet And Bake Sale", "2026 Club Officer Election Results",
        ]
        for title in honest:
            with self.subTest(title=title):
                self.assertFalse(spam.score_text(title).flagged, spam.score_text(title))

    def test_03_an_address_is_scored_by_its_slug_and_a_title_beats_a_dull_slug(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_slug_of]
        # Tests [@ANCHOR: zero_sudo:club_spam_score_entry]
        cases = [
            (f"{HOST}/sic-bo-paysafe-welcome-bonus-australia/", "", True),
            (f"{HOST}/betexpress-casino-no-deposit-bonus-au-2026/", "", True),
            (f"{HOST}/free-live-casino-roulette/", "", True),
            (f"{HOST}/loistokasino-kerro-kaverille-2/", "", True),  # a compound word, a numbered repeat
            (f"{HOST}/weekly-net/", "", False),
            (f"{HOST}/jasper-county-ares-bet-controls-for-august-2022/", "", False),  # a real page of the hacked site: a typo, a year, "ares"
            (f"{HOST}/joplin-hamfest-september-30-thru-october-1-2022/", "", False),
            (f"{HOST}/?p=123", "Free Spins No Deposit Casino", True),
            (f"{HOST}/?feed=xmlsitemap1225", "", False),  # an address with no words scores nothing
            (f"{HOST}/cele-mai-sigure-cazinouri-online-cu-sloturi-fara-depozit-3/", "", True),
            ("not a url at all", "", False),
        ]
        for url, title, expected in cases:
            with self.subTest(url=url):
                self.assertEqual(spam.score_entry(url, title).flagged, expected, spam.score_entry(url, title))
        self.assertEqual(spam.slug_of(f"{HOST}/a-b/c_d.html"), "a-b c_d")

    def test_04_two_amateur_radio_words_hold_a_borderline_score_down(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_score_text]
        plain = spam.score_text("Casino Night")
        branded = spam.score_text("Casino Night ARES Hamfest")
        self.assertTrue(plain.flagged)
        self.assertLess(branded.points, plain.points)
        self.assertFalse(branded.flagged)
        self.assertEqual(branded.ham, 2)


class TestSitemaps(unittest.TestCase):
    def test_10_a_core_index_and_a_urlset_are_read_without_an_xml_parser(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_parse_sitemap]
        kind, entries, total = spam.parse_sitemap(sitemap_index(["wp-sitemap-posts-post-1.xml", "wp-sitemap-users-1.xml"]))
        self.assertEqual((kind, total), ("index", 2))
        self.assertEqual(entries[0][0], f"{HOST}/wp-sitemap-posts-post-1.xml")
        kind, entries, total = spam.parse_sitemap(core_posts_sitemap())
        self.assertEqual(kind, "urlset")
        self.assertEqual(total, len(CORE_CLEAN) + len(CORE_SPAM))
        self.assertEqual(entries[0], (f"{HOST}/weekly-net/", "2014-11-10T10:19:43-05:00"))
        # The spam plugin's spaced layout, and a file that is not a sitemap at all.
        kind, entries, total = spam.parse_sitemap(injected_sitemap(times=1))
        self.assertEqual((kind, total), ("urlset", len(INJECTED_SLUGS)))
        self.assertEqual(spam.parse_sitemap("<html>not a sitemap</html>"), ("", [], 0))
        self.assertEqual(spam.parse_sitemap(None), ("", [], 0))

    def test_11_an_entity_bomb_is_just_text(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_parse_sitemap]
        bomb = (
            '<?xml version="1.0"?><!DOCTYPE lolz [<!ENTITY lol "lol"><!ENTITY lol2 "&lol;&lol;&lol;&lol;&lol;&lol;">]>'
            "<urlset><url><loc>&lol2;</loc></url></urlset>"
        )
        kind, entries, total = spam.parse_sitemap(bomb)
        self.assertEqual((kind, total), ("urlset", 1))
        self.assertLess(len(entries[0][0]), 40)

    def test_12_the_entry_cap_is_honoured_and_the_total_still_counts(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_parse_sitemap]
        xml = urlset([(f"post-{n}", "") for n in range(50)])
        kind, entries, total = spam.parse_sitemap(xml, limit=10)
        self.assertEqual((len(entries), total), (10, 50))

    def test_13_the_spam_plugin_sitemap_address_is_recognised(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_is_spam_feed_sitemap]
        for url, expected in [
            (f"{HOST}/?feed=xmlsitemap1225", True), (f"{HOST}/index.php?feed=xmlsitemap", True),
            (f"{HOST}/wp-sitemap.xml", False), (f"{HOST}/sitemap_index.xml", False), ("", False), (None, False),
        ]:
            with self.subTest(url=url):
                self.assertEqual(spam.is_spam_feed_sitemap(url), expected)

    def test_14_dates_are_ages_and_nonsense_is_unknown(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_age_days]
        for text, expected in [
            ("2026-10-22", 0), ("2026-09-22T10:19:43-05:00", 30), ("2025-10-22", 365), ("-0001-11-2", None),
            ("0001-01-01", None), ("2027-01-01", None), ("yesterday", None), ("", None), (None, None),
            ("2026-02-30", None),
        ]:
            with self.subTest(text=text):
                self.assertEqual(spam.age_days(text, TODAY), expected)

    def test_15_a_stamped_sitemap_says_nothing_about_age(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_dated_entries]
        _kind, stamped, _total = spam.parse_sitemap(injected_sitemap(times=3, stamp="2026-10-18"))
        self.assertTrue(all(age is None for _loc, age in spam.dated_entries(stamped, TODAY)))
        _kind, real, _total = spam.parse_sitemap(core_posts_sitemap())
        ages = [age for _loc, age in spam.dated_entries(real, TODAY)]
        self.assertTrue(all(age is not None for age in ages))
        self.assertGreater(max(ages), 365)
        # A short sitemap is never called stamped, however alike its dates are.
        short = [(f"{HOST}/p{n}/", "2026-10-18") for n in range(5)]
        self.assertEqual([a for _l, a in spam.dated_entries(short, TODAY)], [4] * 5)


class TestHomePage(unittest.TestCase):
    def test_20_the_recent_posts_widget_and_links_of_a_hacked_page_are_read(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_extract_home_evidence]
        # Tests [@ANCHOR: zero_sudo:club_spam_parser_init]
        # Tests [@ANCHOR: zero_sudo:club_spam_parser_start]
        # Tests [@ANCHOR: zero_sudo:club_spam_parser_end]
        # Tests [@ANCHOR: zero_sudo:club_spam_parser_data]
        # Tests [@ANCHOR: zero_sudo:club_spam_join]
        home = spam.extract_home_evidence(home_html(recent=SPAM_RECENT_POSTS), HOST + "/")
        self.assertTrue(home["wordpress"])
        self.assertTrue(home["title"].startswith("Example County ARES"))
        recent = [e for e in home["entries"] if e["s"] == "recent"]
        self.assertEqual(len(recent), 5)
        self.assertEqual(recent[4]["t"], "Bordeaux Casino No Deposit Bonus Codes For Free Spins 2026")
        self.assertTrue(recent[0]["u"].startswith(HOST + "/risultati"))
        self.assertTrue(all(spam.score_entry(e["u"], e["t"]).flagged for e in recent))
        # The honest page's links are all unflagged and the one inline display:none (Cancel reply) is only counted.
        clean = spam.extract_home_evidence(home_html(), HOST + "/")
        self.assertFalse(any(spam.score_entry(e["u"], e["t"]).flagged for e in clean["entries"]))
        self.assertEqual(clean["hidden_links"], 0)  # the Cancel-reply anchor points at the page itself and is left out
        # An in-page anchor is left out, a relative link is made absolute, an outside link is counted as outbound.
        urls = [e["u"] for e in clean["entries"]]
        self.assertNotIn(HOST + "/#respond", urls)
        self.assertEqual(clean["outbound_domains"], 3)  # arrl, weather.gov, machothemes

    def test_21_a_hidden_block_of_casino_links_is_found_and_css_classes_are_not_read(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_extract_home_evidence]
        for style in ("position:absolute;left:-9999px;", "display:none", "visibility:hidden", "font-size:0px;",
                      "text-indent:-9999px", "opacity:0;"):
            with self.subTest(style=style):
                home = spam.extract_home_evidence(
                    home_html(body_extra=hidden_link_block(25, style)), HOST + "/")
                hidden = [e for e in home["entries"] if e["s"] == "hidden"]
                self.assertGreaterEqual(len(hidden), 25)
                self.assertGreaterEqual(sum(spam.score_entry(e["u"], e["t"]).flagged for e in hidden), 25)
        css_only = spam.extract_home_evidence(
            home_html(head_extra="<style>.hide{display:none}</style>", body_extra='<div class="hide"><a href="/x/">x</a></div>'),
            HOST + "/")
        self.assertEqual(css_only["hidden_links"], 0)  # a stylesheet class is not read, only an inline hide

    def test_22_hostile_or_broken_markup_does_not_raise(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_extract_home_evidence]
        for markup in ["", None, "<<<<", "<a href=", "<div><a href='http://x.example.org/'>x", "\x00\x01<a href='/y'>",
                       "<html>" + "<div>" * 5000 + "</html>", "<a href='http://[::1'>bad host</a>"]:
            with self.subTest(markup=str(markup)[:20]):
                home = spam.extract_home_evidence(markup, HOST + "/")
                self.assertIn("entries", home)

    def test_23_a_home_page_that_looks_bad_is_worth_the_sitemaps(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_is_suspicious]
        bad = spam.extract_home_evidence(home_html(recent=SPAM_RECENT_POSTS), HOST + "/")
        good = spam.extract_home_evidence(home_html(), HOST + "/")
        self.assertTrue(spam.is_suspicious(bad, []))
        self.assertFalse(spam.is_suspicious(good, []))
        self.assertFalse(spam.is_suspicious(good, [f"{HOST}/wp-sitemap.xml"]))
        self.assertTrue(spam.is_suspicious(good, [f"{HOST}/?feed=xmlsitemap1225"]))
        self.assertFalse(spam.is_suspicious(None, None))


class TestEvidenceAndDecision(unittest.TestCase):
    def _hacked(self, **kw):
        """The hacked site as the daemon sees it: spam widget, a core sitemap with dated spam posts, two injected sitemaps."""
        return _evidence(
            home_html(recent=SPAM_RECENT_POSTS),
            [_sitemap("core", core_posts_sitemap(spam_times=kw.pop("spam_times", 12))),
             _sitemap("injected", injected_sitemap(times=kw.pop("injected_times", 6)))],
            [f"{HOST}/?feed=xmlsitemap1225", f"{HOST}/?feed=xmlsitemap1301"], **kw)

    def test_30_a_hacked_site_with_old_dated_spam_is_confirmed_from_the_sitemaps_alone(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_build_evidence]
        # Tests [@ANCHOR: zero_sudo:club_spam_evaluate_evidence]
        evidence = self._hacked()
        self.assertEqual(evidence["robots_spam_sitemaps"], 2)
        self.assertTrue(evidence["wordpress"])
        verdict = spam.evaluate_evidence(evidence)
        self.assertEqual(verdict["level"], spam.LEVEL_CONFIRMED, verdict["reasons"])
        self.assertEqual(verdict["basis"], "lastmod")
        self.assertGreaterEqual(verdict["flagged"], 150)
        self.assertGreater(verdict["fraction"], 0.8)
        self.assertGreaterEqual(verdict["on_topic"], 5)
        self.assertEqual(len(verdict["sample"]), 15)
        self.assertTrue(any("robots.txt" in reason for reason in verdict["reasons"]))

    def test_31_the_injected_sitemaps_alone_never_make_the_age_gate_fail(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_evaluate_evidence]
        # Only the stamped sitemap says anything: no usable date, so the gate waits for our own history.
        evidence = _evidence(None, [_sitemap("injected", injected_sitemap(times=6))], [f"{HOST}/?feed=xmlsitemap1225"])
        young = spam.evaluate_evidence(evidence, first_detected_age_days=3)
        self.assertEqual((young["level"], young["dated_flagged"]), (spam.LEVEL_SIGNIFICANT, 0))
        old = spam.evaluate_evidence(evidence, first_detected_age_days=45)
        self.assertEqual((old["level"], old["basis"]), (spam.LEVEL_CONFIRMED, "history"))
        never = spam.evaluate_evidence(evidence, first_detected_age_days=None)
        self.assertEqual(never["level"], spam.LEVEL_SIGNIFICANT)

    def test_32_fresh_spam_is_significant_but_not_persistent_until_it_ages(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_evaluate_evidence]
        fresh = [(slug, (TODAY.replace(day=1)).isoformat()) for slug in INJECTED_SLUGS]
        spread = [(f"{slug}-{n}", f"2026-10-{1 + n % 20:02d}") for n in range(6) for slug in INJECTED_SLUGS]
        evidence = _evidence(None, [_sitemap("core", urlset(fresh + spread + [(s, f"{d}T00:00:00Z") for s, d in CORE_CLEAN]))])
        verdict = spam.evaluate_evidence(evidence)
        self.assertEqual(verdict["level"], spam.LEVEL_SIGNIFICANT, verdict["reasons"])
        self.assertFalse(verdict["persisted"])
        self.assertGreaterEqual(verdict["dated_flagged"], 100)
        self.assertEqual(verdict["aged_flagged"], 0)
        self.assertTrue(any("not yet persistent" in r for r in verdict["reasons"]))

    def test_33_the_persistence_threshold_is_a_table(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_evaluate_evidence]
        # Tests [@ANCHOR: zero_sudo:club_spam_config_from_params]
        def entries(old, young):
            rows = [(f"online-casino-bonus-{n}", "2026-01-05") for n in range(old)]
            rows += [(f"online-casino-slots-{n}", "2026-10-15") for n in range(young)]
            rows += [(slug, f"{day}") for slug, day in CORE_CLEAN]
            return _evidence(None, [_sitemap("core", urlset(rows))])

        cases = [
            # (old spam, young spam, config overrides, history days, expected level, expected basis)
            (40, 0, {}, None, spam.LEVEL_CONFIRMED, "lastmod"),
            (10, 30, {}, None, spam.LEVEL_CONFIRMED, "lastmod"),  # 25 percent of the dated spam is old: exactly the bar
            (9, 31, {}, None, spam.LEVEL_SIGNIFICANT, ""),  # under the bar, no history
            (9, 31, {}, 31, spam.LEVEL_CONFIRMED, "history"),  # under the bar, but we have watched it a month
            (9, 31, {"aged_fraction": "0.2"}, None, spam.LEVEL_CONFIRMED, "lastmod"),
            (0, 40, {"age_days": "7"}, None, spam.LEVEL_CONFIRMED, "lastmod"),  # a shorter threshold, spam 7+ days old
            (3, 3, {}, None, spam.LEVEL_SUSPECT, ""),  # six flagged: under min_flagged
            (40, 0, {"min_flagged": "100"}, None, spam.LEVEL_SUSPECT, ""),
            (40, 0, {"min_fraction": "0.99"}, None, spam.LEVEL_SUSPECT, ""),
        ]
        for old, young, overrides, history, level, basis in cases:
            with self.subTest(old=old, young=young, overrides=overrides, history=history):
                params = {spam.PARAM_PREFIX + k: v for k, v in overrides.items()}
                config = spam.SpamConfig.from_params(params.get)
                verdict = spam.evaluate_evidence(entries(old, young), config, first_detected_age_days=history)
                self.assertEqual((verdict["level"], verdict["basis"]), (level, basis), verdict["reasons"])

    def test_34_an_honest_club_site_is_clean_and_a_few_odd_pages_are_only_suspect(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_evaluate_evidence]
        filler = [(f"club-news-{n}", "2023-05-01") for n in range(400)] + list(CORE_CLEAN)
        honest = _evidence(home_html(), [_sitemap("core", urlset(filler))], [])
        self.assertEqual(spam.evaluate_evidence(honest)["level"], spam.LEVEL_CLEAN)
        cases = [(2, spam.LEVEL_CLEAN), (4, spam.LEVEL_SUSPECT), (12, spam.LEVEL_SUSPECT), (60, spam.LEVEL_SUSPECT)]
        for count, level in cases:
            with self.subTest(spam_pages=count):
                rows = filler + [(f"best-online-casino-bonus-{n}", "2024-01-01") for n in range(count)]
                verdict = spam.evaluate_evidence(_evidence(home_html(), [_sitemap("core", urlset(rows))], []))
                # 12 of 432 and 60 of 480 are under the 30 percent share bar, however many pages that is.
                self.assertEqual(verdict["level"], level, verdict["reasons"])
        rows = filler[:40] + [(f"best-online-casino-bonus-{n}", "2024-01-01") for n in range(60)]
        verdict = spam.evaluate_evidence(_evidence(home_html(), [_sitemap("core", urlset(rows))], []))
        self.assertEqual(verdict["level"], spam.LEVEL_CONFIRMED, verdict["reasons"])

    def test_35_a_hidden_link_farm_on_the_home_page_is_found_without_any_sitemap(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_evaluate_evidence]
        evidence = _evidence(home_html(body_extra=hidden_link_block(40)), [], [])
        verdict = spam.evaluate_evidence(evidence)
        self.assertEqual(verdict["level"], spam.LEVEL_SIGNIFICANT)  # no dates, no history yet
        self.assertEqual(spam.evaluate_evidence(evidence, first_detected_age_days=40)["level"], spam.LEVEL_CONFIRMED)

    def test_36_the_daemon_is_not_believed_blindly(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_evaluate_evidence]
        honest = _evidence(home_html(), [_sitemap("core", core_posts_sitemap(spam=[], spam_times=0))], [])
        lying = dict(honest, flagged_total=4000, known=4100)
        self.assertEqual(spam.evaluate_evidence(lying, first_detected_age_days=99)["level"], spam.LEVEL_CLEAN)
        garbage = [None, {}, {"entries": "x"}, {"entries": [None, 3, {"u": 4}, {"u": "http://a/", "d": True}]},
                   {"entries": [], "known": "many"}, [], "text"]
        for item in garbage:
            with self.subTest(item=str(item)[:30]):
                self.assertEqual(spam.evaluate_evidence(item)["level"], spam.LEVEL_CLEAN)

    def test_37_a_very_large_infestation_is_capped_in_the_payload_but_counted(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_build_evidence]
        # Tests [@ANCHOR: zero_sudo:club_spam_evaluate_evidence]
        rows = [(f"online-casino-slots-{n}", "2025-01-05") for n in range(3000)]
        rows += [(f"club-page-{n}", "2020-01-05") for n in range(400)]
        evidence = _evidence(None, [_sitemap("core", urlset(rows))])
        self.assertLessEqual(len(evidence["entries"]), spam.MAX_EVIDENCE_ENTRIES)
        self.assertEqual(evidence["flagged_total"], 3000)
        self.assertEqual(evidence["known"], 3400)
        verdict = spam.evaluate_evidence(evidence)
        self.assertEqual(verdict["flagged"], 3000)
        self.assertEqual(verdict["level"], spam.LEVEL_CONFIRMED)

    def test_38_another_hosts_sitemap_entries_are_ignored(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_build_evidence]
        foreign = urlset([(f"http://other.example.net/casino-{n}/", "2025-01-01") for n in range(50)])
        evidence = _evidence(None, [_sitemap("core", foreign)])
        self.assertEqual(spam.evaluate_evidence(evidence)["level"], spam.LEVEL_CLEAN)

    def test_39_a_dropped_site_is_restored_only_when_clean_for_long_enough(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_decide_recheck]
        clean = {"level": spam.LEVEL_CLEAN, "fraction": 0.0, "flagged": 0}
        cases = [
            (clean, 40, "restore"), (clean, 30, "restore"), (clean, 29.9, "keep"), (clean, None, "keep"),
            ({"level": spam.LEVEL_SUSPECT, "fraction": 0.01, "flagged": 4}, 60, "restore"),
            ({"level": spam.LEVEL_SUSPECT, "fraction": 0.01, "flagged": 5}, 60, "keep"),  # half of min_flagged
            ({"level": spam.LEVEL_SUSPECT, "fraction": 0.06, "flagged": 2}, 60, "keep"),
            ({"level": spam.LEVEL_SIGNIFICANT, "fraction": 0.5, "flagged": 50}, 60, "keep"),
            ({"level": spam.LEVEL_CONFIRMED, "fraction": 0.9, "flagged": 900}, 90, "keep"),
            ({}, 90, "keep"),
        ]
        for verdict, days, expected in cases:
            with self.subTest(verdict=verdict, days=days):
                self.assertEqual(spam.decide_recheck(verdict, days), expected)

    def test_40_every_setting_has_a_parameter_name_and_a_default(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_config_from_params]
        defaults = spam.SpamConfig()
        self.assertEqual(
            (defaults.min_flagged, defaults.min_fraction, defaults.age_days, defaults.recheck_days, defaults.clean_days),
            (10, 0.30, 30, 30, 30))
        self.assertIn(spam.PARAM_PREFIX + "mode", spam.PARAM_NAMES)
        self.assertEqual(len(spam.PARAM_NAMES), len(set(spam.PARAM_NAMES)))
        # Bad and missing values keep the default; good ones convert.
        config = spam.SpamConfig.from_params({
            spam.PARAM_PREFIX + "age_days": "45", spam.PARAM_PREFIX + "min_fraction": "x", spam.PARAM_PREFIX + "scan_days": False,
        }.get)
        self.assertEqual((config.age_days, config.min_fraction, config.scan_days), (45, 0.30, 30))
        for name in spam.PARAM_NAMES:
            lowered = name.lower()
            self.assertFalse(any(word in lowered for word in ("secret", "key", "password", "token", "auth", "crypt", "cert")), name)


class TestHeadVerification(unittest.TestCase):
    """Bruce's refinement: a handful of flagged addresses are re-checked with HEAD to see whether the spam is still there."""

    def _flagged_evidence(self, n=40):
        rows = [{"u": f"{HOST}/best-online-casino-bonus-{i}/", "t": "", "d": 400 - i * 5, "s": "core"} for i in range(n)]
        rows += [{"u": f"{HOST}/spam-slots-{i}/", "t": "", "d": None, "s": "injected"} for i in range(n)]
        rows += [{"u": f"{HOST}/club-news-{i}/", "t": "", "d": 900, "s": "core"} for i in range(20)]
        return {"entries": rows, "known": len(rows), "flagged_total": 2 * n}

    def test_50_the_sample_is_ten_flagged_urls_spread_out_and_the_oldest_dated_first(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_pick_head_sample]
        verdict = spam.evaluate_evidence(self._flagged_evidence())
        sample = verdict["head_sample"]
        self.assertEqual(len(sample), 10)
        self.assertEqual(len(set(sample)), 10)
        self.assertTrue(all(spam.score_entry(u).flagged for u in sample))
        self.assertTrue(all("club-news" not in u for u in sample))
        # The oldest dated spam is preferred: the first dated addresses are in it.
        self.assertIn(f"{HOST}/best-online-casino-bonus-0/", sample)
        # Few flagged URLs: all of them; none: nothing.
        few = self._flagged_evidence(n=2)
        self.assertEqual(len(spam.evaluate_evidence(few)["head_sample"]), 4)
        self.assertEqual(spam.evaluate_evidence(_evidence(home_html(), [], []))["head_sample"], [])
        self.assertEqual(spam.pick_head_sample([], 10), [])

    def test_51_head_answers_are_judged_still_spam_removed_or_unknown(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_evaluate_head_results]
        def answers(*codes, **extra):
            return [dict({"u": f"{HOST}/p{i}/", "status": code}, **extra) for i, code in enumerate(codes)]

        cases = [
            (answers(200, 200, 200, 200, 404), "still_spam"),
            (answers(200, 200, 404, 404), "still_spam"),  # at least half answer 2xx
            (answers(200, 404, 404, 404), "removed"),
            (answers(404, 410, 404, 410), "removed"),
            (answers(405, 405, 403, None), "unknown"),  # never "removed"
            (answers(200, 405, 403, 500), "unknown"),
            (answers(404, 405, 405, 405), "unknown"),
            ([{"u": "a", "status": 301, "redirect_home": True}] * 4, "removed"),
            ([{"u": "a", "status": 301, "redirect_home": False}] * 4, "unknown"),
            ([], "unknown"),
        ]
        for results, expected in cases:
            with self.subTest(results=results[:2], expected=expected):
                self.assertEqual(spam.evaluate_head_results(results, TODAY)["result"], expected)

    def test_52_an_old_last_modified_or_a_kept_etag_counts_as_persistence(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_evaluate_head_results]
        # Tests [@ANCHOR: zero_sudo:club_spam_http_date_age]
        old = "Mon, 01 Sep 2025 10:00:00 GMT"
        young = "Mon, 19 Oct 2026 10:00:00 GMT"
        rows = [{"u": f"{HOST}/p{i}/", "status": 200, "last_modified": old if i < 4 else young} for i in range(6)]
        judged = spam.evaluate_head_results(rows, TODAY)
        self.assertEqual((judged["alive"], judged["old_count"], judged["with_validators"]), (6, 4, 6))
        self.assertGreater(spam.http_date_age("Mon, 01 Sep 2025 10:00:00 GMT", TODAY), 300)
        for bad in ("", None, "yesterday", "Mon, 99 Foo 2025 10:00:00 GMT"):
            self.assertIsNone(spam.http_date_age(bad, TODAY))
        self.assertIsNone(spam.http_date_age("Mon, 01 Sep 2099 10:00:00 GMT", TODAY))
        # An ETag alone says nothing about age; it is only counted.
        judged = spam.evaluate_head_results([{"u": "a", "status": 200, "etag": '"x"'}] * 2, TODAY)
        self.assertEqual((judged["old_count"], judged["with_validators"]), (0, 2))

    def test_53_spam_blocks_are_cut_from_a_page_before_a_model_sees_it(self):
        # Tests [@ANCHOR: zero_sudo:club_spam_strip_spam_blocks]
        # Tests [@ANCHOR: zero_sudo:club_spam_strip_init]
        # Tests [@ANCHOR: zero_sudo:club_spam_strip_offset]
        # Tests [@ANCHOR: zero_sudo:club_spam_strip_start]
        # Tests [@ANCHOR: zero_sudo:club_spam_strip_data]
        # Tests [@ANCHOR: zero_sudo:club_spam_strip_end]
        page = home_html(recent=SPAM_RECENT_POSTS)
        text, removed = spam.strip_spam_blocks(page, HOST + "/")
        self.assertGreater(removed, 300)
        self.assertNotIn("Casino", text)
        self.assertNotIn("Blackjack", text)
        self.assertIn("Amateur Radio Emergency Service of Example County", text)
        self.assertIn("146.940", text)
        self.assertIn("Frequencies / Nets", text)
        # A clean page is returned untouched, and a hidden link farm is cut.
        clean = home_html()
        self.assertEqual(spam.strip_spam_blocks(clean, HOST + "/"), (clean, 0))
        farm, removed = spam.strip_spam_blocks(home_html(body_extra=hidden_link_block(30)), HOST + "/")
        self.assertNotIn("free spins", farm)
        self.assertGreater(removed, 1000)
        self.assertEqual(spam.strip_spam_blocks("", HOST + "/"), ("", 0))


if __name__ == "__main__":
    unittest.main()
