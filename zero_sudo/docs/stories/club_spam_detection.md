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

## Reading Sitemaps Without an XML Parser `[@ANCHOR: zero_sudo:club_spam_parse_sitemap]`

`parse_sitemap(text)` reads a sitemap index or URL set with regular expressions and returns its kind, up to 5,000 entries
(address and last-modified text) and the true entry count. A hostile site could send an entity bomb; a regular expression has no
entities to expand. `is_spam_feed_sitemap(url)` `[@ANCHOR: zero_sudo:club_spam_is_spam_feed_sitemap]` recognises the address
shape the spam plugins inject into `robots.txt` (`/?feed=xmlsitemap1225`), which no legitimate sitemap plugin writes.

## How Old Is the Spam: Not From the ETag `[@ANCHOR: zero_sudo:club_spam_age_days]`

A WordPress page sends neither an `ETag` nor a `Last-Modified` header (checked live on the site above, spam pages and real ones
alike), so the server gives no validator to read. The age of a flagged address is taken from the sitemap's `<lastmod>`, which
WordPress core fills with the post's modified time. `age_days(text, today)` turns it into whole days and returns None for text
that is not a date, a year before 1995 (WordPress writes year -1 for an undated post) or a date in the future.
`dated_entries(entries, today)` `[@ANCHOR: zero_sudo:club_spam_dated_entries]` treats a sitemap whose dates are one day for nine
in ten of at least twenty URLs as STAMPED: the spam plugin's extra sitemaps carry the day the file was generated, which says
nothing about the pages, so their ages are None and cannot make the age gate fail. When too few flagged addresses have a usable
date, our own history decides: the time the site was first seen significantly spammed and not clean since.

## Reading a Home Page `[@ANCHOR: zero_sudo:club_spam_extract_home_evidence]`

`extract_home_evidence(markup, url)` reads a home page with the standard library parser and returns whether it looks like
WordPress, its title, every link (with the "recent posts" widget and anything inside an inline-styled hidden element marked),
and counts of hidden and outbound links. A hide is recognised only in an inline `style` (`display:none`,
`visibility:hidden`, a font size of 0 to 2 pixels, a large negative indent or offset, zero opacity): stylesheet classes are
common in honest themes and are not read. A link to the page itself (the comment form's `#respond`) is left out. The parser
class is a stack of open tags with two inherited flags: `__init__` `[@ANCHOR: zero_sudo:club_spam_parser_init]`,
`handle_starttag` `[@ANCHOR: zero_sudo:club_spam_parser_start]`, `handle_endtag` `[@ANCHOR: zero_sudo:club_spam_parser_end]` and
`handle_data` `[@ANCHOR: zero_sudo:club_spam_parser_data]`; relative links are made absolute by `_join()`
`[@ANCHOR: zero_sudo:club_spam_join]`.

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
