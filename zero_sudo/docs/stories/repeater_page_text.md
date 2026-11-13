<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: A Repeater's Callsign Is the One the Page Ties to the Repeater

As a **Club Webmaster**, I title my page "K&Oslash;HNY/R" and say "EchoLink node 469520 operated by KC&Oslash;REO", and I want hams.com to list the
repeater as K0HNY and not as the node operator's callsign or with a "/R" the FCC (US Federal Communications Commission) never licensed. As an **Administrator**, I want to
know when a page is old.

## The Process

All of it is in `zero_sudo/daemon/repeater_page_text.py`, a pure-stdlib file that imports nothing from `odoo` (Odoo imports it as
`odoo.addons.zero_sudo.daemon.repeater_page_text`, the crawl daemons import the flat copy beside `glyph_normalizer.py`, which it builds on).
Every public function never raises: bad input gives the neutral value (`''`, `None`, `set()`, or the input unchanged). The page text is the
cleaned text of `polite_fetcher.clean_for_model`: one line per block-level element, one line per table row, cells joined by `" | "`.

1. A trailing repeater designator (/R, -R, /RPT, -RPT, /REP, " RPT") on a callsign-shaped token is removed from the repeater's callsign; the
   page's own text is kept in the notes (`callsign field as extracted: K&Oslash;HNY/R`). /P, /M, /QRP and country prefixes such as W1AW/VE3 are never touched.
2. A callsign the page names as the operator, owner or sponsor of an EchoLink, AllStar or IRLP node ("node 469520 operated by KC0REO") is not
   accepted as the repeater's callsign when the page names another callsign.
3. A page that states its own date behind a label ("Last updated 14-Jan-2021", "Revised ...", "as of ..."), more than two years old, gets the
   note "page dated 14-Jan-2021, may be out of date". It is a note, never a rejection.
4. The page's structure decides whose a repeater is, in code, whatever the model said. Headings such as 'Other Local Repeaters', 'Local and Regional Repeaters',
   'Useful ... Repeaters' or 'KERN Linked Repeater System' set the repeaters under them apart; a callsign taken from another block is removed; the club's own
   callsign (the title bar, the footer, with an APRS (Automatic Packet Reporting System) suffix or /R ignored), a club section, 'our' or 'operated by <the club>' are club evidence; a list of several
   licensees with none of that is 'unclear'.
5. A repeater whose frequency is not on the page was invented, and a page that is only an unedited default page has none to list.
6. A "repeater" in orbit (the ISS cross-band repeater, an amateur satellite) is recognised from the item's own name, section, address, callsign (NA1SS, RS0ISS, ...)
   or an uplink/downlink description; a terrestrial repeater that only mentions satellites, ISS inside a longer word, a club that makes ARISS contacts, or a club
   called OSCAR on 145.490 is not.

The entries follow in the order of the file: callsigns, page date, where on the page, grounding, placeholder pages, ownership, orbit. Each
public function says what it returns; the `_` helpers are listed with the function that uses them.

## Callsigns

### strip_repeater_designator `[@ANCHOR: zero_sudo:repeater_page_text_strip_repeater_designator]`

**Signature.** `strip_repeater_designator(callsign) -> (str, str)`. `callsign` is any value; a non-string reads as `''`.

**Returns.** `(callsign, suffix)`: the callsign stripped of whitespace and upper-cased, with a trailing repeater designator removed, and the
designator that was removed (`''` when none). The designator is `/R`, `-R`, `/RPT`, `-RPT`, `/REP`, `-REP`, or a space followed by `RPT` or `REP`, and is
removed only when what is left is callsign-shaped (one to three letters or digits, one digit, one to four letters). `/P`, `/M`, `/QRP`, country
prefixes (`VE3/W1AW`) and anything not callsign-shaped come back as typed (upper-cased). The caller stores the page's own text in the notes. No glyph
repair here (`_clean` is applied by the callers that need it). Pure; no errors.

### _clean `[@ANCHOR: zero_sudo:repeater_page_text_clean]`

`_clean(text) -> str` returns `glyph_normalizer.normalize_callsign_text(text)[0]` for a `str`, else `''`. It is the single place where page text
gets its slashed zeros, full-width digits and zero-width characters repaired (and homoglyph letters inside callsign-shaped tokens), so every
callsign comparison in this file works on repaired text. The change list of the normalizer is dropped here; callers that need it call the
normalizer directly. Pure.

### node_operator_callsigns `[@ANCHOR: zero_sudo:repeater_page_text_node_operator_callsigns]`

`node_operator_callsigns(text) -> set[str]` returns the callsigns that `text` names as the operator, owner or sponsor of an EchoLink, AllStar or
IRLP node. Three patterns are tried, case-insensitively, on the glyph-repaired text: "node 469520 operated/run/owned/sponsored/maintained/hosted/provided by
KC0REO" (node number of 3 to 7 digits), "node operator/owner/sysop/sponsor is: W1AW", and "W1AW runs/operates/hosts/sponsors/maintains the node". Each hit is
upper-cased with a repeater designator removed. An empty set when `text` is not a string or has none. The callers use the set to refuse such a callsign as
the repeater's own when the page ties another callsign to it. Pure.

### callsigns_in_text `[@ANCHOR: zero_sudo:repeater_page_text_callsigns_in_text]`

`callsigns_in_text(text) -> set[str]` returns every callsign-shaped token in the glyph-repaired `text` (one to three letters or digits, a digit, one to four
letters, optionally followed by `/R`, `-R`, `/RPT` or `/REP`), upper-cased with that designator removed. A token glued to other letters, digits, `/`, `.` or `-`
is not matched, which keeps frequencies, email addresses and longer words out. The match is loose on purpose: it answers whether a page names any callsign
besides a node operator's, and is the callsign finder used by the adjacency, header and grounding checks. An empty set for a non-string. Pure.

## When the page was written

### _month `[@ANCHOR: zero_sudo:repeater_page_text_month]`

`_month(name) -> int` maps a month name or abbreviation to 1 to 12 using its first three letters, case-insensitively (`Jan`, `january`, `SEPT`). The
caller's regular expression has already matched a month word, so every call is valid; a string that is not a month raises `ValueError` from
`list.index`, which `_to_date` catches. Pure.

### _to_date `[@ANCHOR: zero_sudo:repeater_page_text_to_date]`

`_to_date(kind, parts) -> date | None` builds a `datetime.date` from the groups of one date match. `kind` is `dmy` (day, month name, year), `mdy` (month name,
day, year), `iso` (year, month, day) or `us` (month/day/year), `parts` the matched strings. It returns `None` for a calendar-impossible date (a 31 February,
month 13), catching the `ValueError`. Month/day/year is always read as US order. Pure.

### page_date `[@ANCHOR: zero_sudo:repeater_page_text_page_date]`

**Signature.** `page_date(text) -> (date | None, str)`. `text` is the page text.

**Returns.** The latest date the page states of ITSELF and the text of that date as written. Only a date behind a label counts: `last updated`, `updated`,
`revised`, `revision`, `modified`, `as of`, `date`, `dated`, optionally followed by `on`, `:` or `-`. The date may be day-month-name-year (`14-Jan-2021`,
`3rd January 2019`), month-name-day-year, `2022-05-06` or `5/6/2022`. A date with no label (an event, a copyright year) is ignored. `(None, '')` when there is none or none is
a real date. Digits of any script are repaired first with `normalize_number_text`. Pure; the latest of several labelled dates wins.

### stale_page_note `[@ANCHOR: zero_sudo:repeater_page_text_stale_page_note]`

`stale_page_note(text, today=None) -> str` returns `page dated <date as written>, may be out of date` when `page_date(text)` finds a date more than
`STALE_AFTER` (two years, 730 days) before `today` (a `datetime.date`, default `date.today()`), else `''`. The caller appends the note to the repeater's
notes. It is only a note: the import never rejects an old page for this. Pure apart from reading the clock.

## Where on the page a repeater stands

The headings that set repeaters apart from the club's own are English, German, Spanish, French and Italian patterns; a "weak" heading ("Area Repeaters",
"Repeaters nearby") counts as another club's only on a page that also has a club section of its own, because a page whose only list is headed "Area
Repeaters" is the club's list.

### _heading_kind `[@ANCHOR: zero_sudo:repeater_page_text_heading_kind]`

`_heading_kind(line) -> str | None` classifies one line of page text as `'other'` (a strong other-club heading), `'weak_other'`, `'club'` (our/my/club/member
repeaters, or a short title containing "repeater" with at least two words that does not start like a sentence: the, a, we, if, please...) or `None`. Only a
short line (at most 80 characters, 60 for club) without ` | `, a colon, or sentence-ending punctuation can be a heading, so a lone word `Repeaters` (a menu entry or page
title) and a sentence are `None`. Leading and trailing `* # -`, dashes and a trailing colon are ignored. Pure.

### _title `[@ANCHOR: zero_sudo:repeater_page_text_title]`

`_title(line) -> str` is a heading line reduced to its lower-case text: leading and trailing `* # -`, en and em dashes and tabs removed, then a trailing colon, then
whitespace. Used to test a heading against the "our repeaters" pattern and to report the heading text. Pure.

### _frequencies `[@ANCHOR: zero_sudo:repeater_page_text_frequencies]`

`_frequencies(line) -> list[float]` returns every decimal number of the form `NN.N` to `NNNN.NNNNN` in the line (2 to 4 integer digits, 1 to 5 decimals, not
part of a longer number, and not followed by `Hz`), as floats in MHz, in order of appearance. It does not decide that a number is a frequency, only that it has a
frequency's shape. It is used for line scanning and by `placeholder_page` and `orbiting_repeater`. Pure.

### _structure `[@ANCHOR: zero_sudo:repeater_page_text_structure]`

`_structure(page_text) -> (lines, kinds, scope, heading_at, detail, own)`, memoised with `functools.lru_cache(maxsize=8)` (so `page_text` must be hashable,
a `str`). `lines` is the text split on newline. `kinds` gives each line `'row'` (contains ` | `), `'other'`, `'club'`, `'weak_other'` or `'text'`. `scope` gives each
line the scope of the nearest heading above it (`'club'`, `'other'` or `None`) and `heading_at` that heading's line index. `detail` is the set of club headings
whose block has a `Call Sign:` line (a detail block for one repeater), and `own` the club headings that are really the club's (an "our repeaters" title or a detail
block). If `own` is empty, weak other-club headings are demoted to text, and the scopes are recomputed. The result is shared between calls, so callers
must not modify it. Pure.

### _occurrences `[@ANCHOR: zero_sudo:repeater_page_text_occurrences]`

`_occurrences(page_text, freq) -> list[int]` returns the indexes of the lines that contain a frequency within `_FREQUENCY_TOLERANCE` (0.00051 MHz) of `freq`
(MHz). Empty when the frequency does not appear. It is the way every structural check locates a repeater on the page, and it uses `_structure`'s cached lines. Pure.

### section_scope_for_frequency `[@ANCHOR: zero_sudo:repeater_page_text_section_scope_for_frequency]`

**Signature.** `section_scope_for_frequency(page_text, freq) -> (str | None, str)`

| Argument | Type | Meaning |
|---|---|---|
| `page_text` | `str` | Cleaned page text. |
| `freq` | `int` or `float` | The repeater's frequency in MHz (a bool is refused). |

**Returns.** `('other', heading)` when the page lists the frequency under a heading that sets it apart from the club's own repeaters, `('club', heading)` when it stands in
a section of the club's own (an "Our Repeaters" heading, or a titled block with a `Call Sign:` line), else `(None, '')`. When the frequency is in two places the more
specific wins (its own table row or a detail block beats a heading); a tie, or a generic club title that does not say whose the repeaters are ("Tehachapi Area
Repeaters"), gives `(None, '')` and is left to the model. Wrong types give `(None, '')`. Pure.

### _own_span `[@ANCHOR: zero_sudo:repeater_page_text_own_span]`

`_own_span(page_text, occurrences) -> set[int]` returns the line indexes that belong to a repeater: each occurrence line itself, the heading of its section, the whole block when the
section is a detail block (not a table row), and up to three following plain text lines with no frequency (the section's intro). A callsign found there is the
repeater's own. `occurrences` is the result of `_occurrences`. Pure.

### callsign_adjacent `[@ANCHOR: zero_sudo:repeater_page_text_callsign_adjacent]`

**Signature.** `callsign_adjacent(page_text, callsign, freq) -> bool | None`

**Returns.** `True` when the callsign (glyph-repaired, designator removed) is in the repeater's own span (its row or block, or the heading or intro of its section) or on a line
that belongs to no repeater block (a page header or footer, the organisation's callsign). `False` when it is found only inside some other repeater's row, block or section
(the model borrowed a callsign from a different table). `None` when the frequency is not on the page, the callsign is not on the page, or an argument has the wrong type:
nothing can be said. The import removes the callsign and adds the note "callsign from another section removed" on `False`. Pure; whitespace-tolerant.

## Grounding: is it on the page?

A repeater the model returns from a page that does not state its frequency was invented (an unedited WordPress sample page produced three repeaters). Numbers are
compared as numbers: 147.93, 147.930, 147,93 and 147930 (kHz) are the same.

### _page_numbers `[@ANCHOR: zero_sudo:repeater_page_text_page_numbers]`

`_page_numbers(page_text) -> tuple[float, ...]`, memoised (`lru_cache(maxsize=8)`), returns every number the page states, sorted ascending, in each reading a frequency might take:
as written, a single comma read as a decimal comma, commas removed as thousands separators, a value from 1,000 up to but excluding 10,000,000 also divided by 1,000 (kHz to MHz), and a
value of 10,000,000 or more divided by 1,000,000 (Hz to MHz). Digits are repaired with `normalize_number_text`. Non-finite values (a page "number" of hundreds
of digits) are dropped. A non-string gives an empty tuple. The tuple is shared, never modified. Pure.

### _near `[@ANCHOR: zero_sudo:repeater_page_text_near]`

`_near(values, wanted, tolerance) -> bool` is True when the sorted sequence `values` has an element within `tolerance` of `wanted`, by `bisect` in O(log n). `values` must be sorted
(`_page_numbers` returns it so). It does the comparison for `frequency_on_page` and `number_on_page`. Pure.

### frequency_on_page `[@ANCHOR: zero_sudo:repeater_page_text_frequency_on_page]`

`frequency_on_page(page_text, freq) -> bool` is True when the page states `freq` (MHz) to within 0.00051 MHz, in any of the readings of `_page_numbers` (147.93, 147.9300, `147,93`, 147930 kHz).
A non-number `freq` (including a bool) gives False. Pure; never raises.

### number_on_page `[@ANCHOR: zero_sudo:repeater_page_text_number_on_page]`

`number_on_page(page_text, value, tolerance=0.06) -> bool` is True when the page states a number within `tolerance` of `value`. The default 0.06 suits a CTCSS (continuous tone-coded squelch system) tone (such as 107.2 or 100.0). `grounded_notes` uses 0.001
for amounts. A non-number `value` gives False. Pure; never raises.

### grounded_notes `[@ANCHOR: zero_sudo:repeater_page_text_grounded_notes]`

**Signature.** `grounded_notes(notes, page_text) -> (str, list[str])`

**Returns.** `(kept, dropped)`. `notes` is split into sentences; a sentence is dropped when it makes a specific claim the page does not back: an amount with a unit
(W, watts, kW, miles, km, feet, meters) whose number is not on the page, or an agency or service from a fixed list (Navy, Army, Coast Guard, FEMA, Red Cross, NOAA: National Oceanic and Atmospheric Administration,
Skywarn, Sheriff, Police, Fire Department, Hospital...) that the page never names. Any other sentence is paraphrase and is kept. `kept` is the kept sentences joined by a space, `dropped` the removed sentences.
`("", [])` for empty or non-string notes. Pure.

### ground_item `[@ANCHOR: zero_sudo:repeater_page_text_ground_item]`

**Signature.** `ground_item(item, page_text) -> (dict, list[str])`. `item` is one extracted repeater dict (not modified; a copy is returned), `page_text` the cleaned page text.

**Returns.** `(item, problems)`. If the item's `output_frequency` is a number the page does not state, the item gets `not_on_page` set to a plain reason, loses its `notes`, and
the only problem is the frequency (it was not read from this page). Otherwise an `input_frequency`, `tone_uplink`, `tone_downlink` or `callsign` the page does not
state is removed, and `notes` sentences that `grounded_notes` drops are removed (the key too if none remain). `problems` is a list of readable lines to append to the item's notes. A non-dict
is returned unchanged with `[]`. A callsign is checked after repair and designator removal, only when it is callsign-shaped. Pure; never raises.

## Placeholder pages

### placeholder_page `[@ANCHOR: zero_sudo:repeater_page_text_placeholder_page]`

**Signature.** `placeholder_page(page_text) -> str`. **Returns** the first placeholder phrase found (such as `lorem ipsum`, `this is an example page`, `welcome to nginx`, `under
construction`, `domain parked`), or `''` for a real page. A page is a placeholder only when a placeholder phrase appears and no line states a frequency, and either the
whole main text (lines of 30 or more characters that are not footers: copyright, powered by, all rights reserved, privacy policy) is 160 characters or fewer, or the lines
carrying placeholder text are half or more of the main text by length. A page that mentions "under construction" in real content is therefore not a placeholder. Non-string
gives `''`. The caller extracts no repeaters from such a page. Pure.

## Whose is it? Ownership

### callsign_core `[@ANCHOR: zero_sudo:repeater_page_text_callsign_core]`

`callsign_core(callsign) -> str` returns a callsign for COMPARISON only: upper-case, glyph-repaired, repeater designator removed and an APRS SSID (`-1` to `-15`) removed, so `AC6EE-3` and
`ac6ee/R` are both `AC6EE`. What the page wrote is what gets stored; this value is never stored. A non-string gives `''`. Pure.

### header_callsigns `[@ANCHOR: zero_sudo:repeater_page_text_header_callsigns]`

`header_callsigns(page_text) -> set[str]` returns the callsigns (as `callsign_core`) that the page states as its OWN: those that appear in the first five non-empty lines and again in the
last eight (a title bar `TARA AC6EE`, a footer `73 de AC6EE`, a copyright line). Lines that are table rows are ignored, so a callsign in a row or block is not the club's. A page of fewer than 12 non-empty lines gives an empty set, as
does a non-string. Pure.

### _title_words `[@ANCHOR: zero_sudo:repeater_page_text_title_words]`

`_title_words(page_text) -> set[str]` returns the lower-case words (three or more letters) of the first four non-empty lines that are not table rows and have no frequency and are under 90 characters
(the page title and tagline), minus generic words (the, club, amateur, radio, repeater, county, valley, association...). It is the club's name as the page writes it, used to
decide whether "operated by <someone>" names the club itself. Pure.

### row_ownership `[@ANCHOR: zero_sudo:repeater_page_text_row_ownership]`

**Signature.** `row_ownership(page_text, freq, callsign=None) -> str | None`

**Returns.** `'club'`, `'other'` or `None` (no evidence) for the repeater on `freq`, from the page alone. First `section_scope_for_frequency`: 'club' or 'other' from a heading wins. Then, from
the repeater's own span: 'club' when its callsign is one of `header_callsigns`, or the span says "our" or "we own/operate/maintain/run/sponsor"; and for "operated/owned/maintained/sponsored/run by X", 'club' when X
holds a header callsign or shares a title word with the page, else 'other'. A list that has only a generic title gives `None`. A non-string page gives `None`. Pure.

### _append_note `[@ANCHOR: zero_sudo:repeater_page_text_append_note]`

`_append_note(item, text)` mutates the dict `item` in place: its `notes` becomes `"<text>. <old notes>"` when it already had non-empty string notes, else just `text`. Returns nothing. Used by `apply_page_checks` to record
the "callsign from another section removed" note in front of what the model wrote. No locks.

### _item_frequency `[@ANCHOR: zero_sudo:repeater_page_text_item_frequency]`

`_item_frequency(item, page_text) -> float | None` returns the first of the item's `output_frequency` and `input_frequency` that is a number (not a bool) which occurs on the page (`_occurrences` is non-empty),
as a float, else `None`. It chooses the frequency used to find the repeater's position on the page. Pure.

### apply_page_checks `[@ANCHOR: zero_sudo:repeater_page_text_apply_page_checks]`

**Signature.** `apply_page_checks(items, page_text) -> list`. `items` is the list of extracted repeater dicts (not modified; each dict is copied), `page_text` the cleaned page text.

**Returns.** The list with what the page's own structure says, decided in code whatever the model returned. For each dict with a frequency on the page: `listing_scope` is set to `'other'` when
the page lists it under a heading that sets it apart (the model may make this stricter, club to other, never looser), or to `'club'` when it stands in the club's own section and the model gave none; the section
heading is put in `listing_section`; a callsign that `callsign_adjacent` says is only in another repeater's block is removed and the note `callsign from another section removed: <callsign>` is added. Then every item with a
callsign gets `listing_ownership`: `'club'` or `'other'` from `row_ownership` (an item whose `listing_scope` is 'other' is 'other'), else `'unclear'` when two or more distinct callsigns have no evidence, else `'unspecified'`.
Items that are not dicts pass through; a non-string `page_text` returns `items` unchanged. Pure; never raises.

## Repeaters in orbit

### orbiting_repeater `[@ANCHOR: zero_sudo:repeater_page_text_orbiting_repeater]`

**Signature.** `orbiting_repeater(name=None, notes=None, section=None, url=None, callsign=None, frequencies=()) -> str`. All arguments are optional and any may be `None`; `frequencies` is accepted for the caller's convenience and is not read
(unknown: to-do, remove it or use it for the ISS frequency test, which now reads the frequencies out of the notes).

**Returns.** The text that shows the item is IN ORBIT (the ISS cross-band repeater, an amateur satellite or its transponder), or `''`. Evidence, in order: a callsign in `ISS_CALLSIGNS` (NA1SS, RS0ISS, OR4ISS, DP0ISS, IR0ISS,
LX0ISS, W5RRR, N1SS) returns it; then, in the name, section and the words of the source address, a specific term: ARISS, ISS as a whole word, "International Space Station" and its translations, satellite designators (AO-91, SO-50,
RS-44, XW-2A, CAS-4, LO-, EO-), QO-100, Es'hail, Tianhe, UKube, LilacSat, CubeSat, or OSCAR with a number up to 129 (never `OSCAR 145.490`, a frequency); the plain word `satellite` counts there only beside uplink, downlink, transponder, orbit, amsat, linear or cross-band. In the notes only a description counts: the word uplink or downlink plus a specific term, or the
word satellite, or two or more frequencies among the ISS frequencies (145.990, 145.800, 145.825, 437.800, 437.550). A terrestrial repeater whose notes mention "FM satellite format", or a club that makes ARISS contacts, is not rejected.
Pure; never raises.

## Verification

`zero_sudo/tests/test_repeater_page_text.py` is a `BaseCase` suite (no database) built from the text of a real club page. The tests that carry anchors:

### test_a_repeater_designator_suffix_is_stripped_from_a_callsign_shaped_token_only `[@ANCHOR: zero_sudo:test_repeater_page_text_designator]`

Covers `strip_repeater_designator`. It checks that `K0HNY/R`, `K0HNY-R`, `K0HNY/RPT`, `K0HNY RPT`, lower-case `w5awp-r` and `WA9RTI/REP` give the bare callsign and the exact
suffix, and that `W1AW/P`, `/M`, `/QRP`, `W1AW/VE3`, `VE3/W1AW`, `W1AW/0`, `N/A`, `REPEATER`, `R`, `/R`, `ABC/R` and the empty string come back unchanged apart from upper-casing,
with an empty suffix. `None` gives `("", "")`.

### test_the_operator_of_an_echolink_allstar_or_irlp_node_is_found_even_with_a_slashed_zero `[@ANCHOR: zero_sudo:test_repeater_page_text_node_operators]`

Covers `node_operator_callsigns` (and, in the neighbouring test, `callsigns_in_text`). The page fixture gives `{"KC0REO"}`; "run by", "maintained by", "sponsored by"
with a slashed zero, "Node operator:" and the lower-case callsign are found; "node 469520 is linked to the W1AW repeater", "Trustee: K0HNY", the empty string and `None` give
the empty set.

### test_a_page_that_states_its_own_old_date_gets_a_note_and_a_recent_or_undated_one_does_not `[@ANCHOR: zero_sudo:test_repeater_page_text_stale]`

Covers `page_date` and `stale_page_note` with a fixed `today` of 2026-10-22. It checks the fixture page date `14-Jan-2021` and its note, the label forms (Revised, modified, `as of 5/6/2022`,
Updated 3 March 2023), that the latest of several stated dates wins, that a date under two years old and exactly two years old gives no note, that a copyright year
and impossible dates (31 February, 99/99) are not page dates, and that `None` is accepted.

The same file also holds the section, adjacency, grounding, placeholder, ownership and orbit tests (anchors `test_repeater_page_text_sections`, `_adjacency`, `_grounding`,
`_placeholder`, `_ownership`, `_orbiting`), each of which cites the functions above from its code.
