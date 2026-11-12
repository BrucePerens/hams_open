<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: A Callsign or a Number Is the Same Whatever Glyphs the Page Used

As a **Club Webmaster**, I write my club's callsign the way hams write it: with a slashed zero (W&Oslash;AZR), sometimes with
full-width digits, a non-breaking space or a crossed seven, and I want hams.com to read that as W0AZR and not to throw my
repeaters away as "no callsign". As an **Administrator**, I want a page to be unable to turn look-alike characters into a
different callsign without anyone noticing, and I want a callsign that is plain mistyped (a letter O for a zero, an S for a 5)
sent to a person to correct and not guessed.

## The Process

1. A page's callsign text goes through `normalize_callsign_text()` (see the entry below). Zero-width and other format characters are removed and every
   space separator becomes a plain space; compatibility forms (full-width, circled, superscript, mathematical digits) become
   ASCII; every decimal digit of any script becomes its ASCII digit; a combining stroke, slash or dot on a digit (a crossed seven,
   0 with a slash, a dotted zero) is removed and a letter O carrying a stroke counts as a slashed zero.
2. A zero-like glyph (O with stroke, empty set, diameter sign, theta, barred O, phi) is read as 0, and a Greek or Cyrillic
   capital identical to a Latin letter as that letter, ONLY inside a token that is then the shape of a callsign, and a zero
   only where the shape needs a digit. A Danish place name is not touched. O/0, I/l/1, S/5, B/8 and Z/2 typed in ASCII are never
   converted.
3. The call returns the list of every substitution (the glyph, its code point, what replaced it) so the caller writes it into the
   candidate's notes, and a repaired callsign always says what was repaired.
4. A number goes through `normalize_number_text()` (see the entry below): digits of any
   script, full-width and Arabic decimal and thousands separators, and the Unicode minus become ASCII, and a zero-like glyph
   between digits is a zero. The caller decides what a comma means.

## Public Functions

Both live in `zero_sudo/daemon/glyph_normalizer.py`, a pure-stdlib file that must not import `odoo`: Odoo imports it as
`odoo.addons.zero_sudo.daemon.glyph_normalizer` and the standalone crawl daemons import the flat copy that
`devbox_tools/deploy_to_production.py` syncs beside `page_cleaning.py`. Both return `(clean_text, changes)`, where `changes` is
a list of `(original, "U+XXXX", replacement)` tuples, one per substitution or removal (`replacement` is `''` for a removal).

### normalize_callsign_text `[@ANCHOR: zero_sudo:glyph_normalizer_normalize_callsign_text]`

**Purpose.** Repairs the digits, spaces and look-alike letters of text that may hold amateur callsigns, so that W&Oslash;AZR is read as
W0AZR, without ever turning look-alikes into a different callsign unnoticed. Called by the Odoo import code and the crawl daemons
(through `repeater_page_text.py`) on page text before a callsign is validated.

**Signature.** `normalize_callsign_text(text) -> (str, list[tuple[str, str, str]])`

| Argument | Type | Meaning |
|---|---|---|
| `text` | `str` | Any page text, a single callsign or a whole line. Not modified (strings are immutable). Anything that is not a `str` is treated as empty. |

**Returns.** `(clean_text, changes)`. ASCII input is returned as the very same object with `[]` at once, with no per-character work.
Otherwise `clean_text` is the text after four general stages and one context-bound stage. (1) Format characters (category Cf: zero-width,
BOM, soft hyphen) are removed and each space separator (Zs, Zl, Zp) becomes a plain space. (2) NFKC per character (full-width, ligatures,
superscript, circled, mathematical). (3) Every decimal digit of any script, and every digit-valued No character, becomes its ASCII digit.
(4) A combining mark directly after an ASCII digit is dropped (crossed seven, dotted zero), and a letter O followed by a stroke
overlay U+0335 to U+0338 becomes a slashed zero. Then the context rule: the text is split on plain spaces and, in each token (surrounding
punctuation `.,;:()[]"'` ignored), a zero-like glyph (`ZERO_LIKE`: O with stroke, empty set, diameter sign, theta, barred O, phi and
relatives) becomes `0`, and a Greek or Cyrillic capital identical to a Latin letter (`HOMOGLYPH_LETTERS`) becomes that letter,
**only if** the token, once replaced, is single-case and matches `_CALLSIGN_SHAPE` (prefix of one or two letters, or digit-and-letter, one
digit, one to four letters, optional `/` or `-` suffix of one to four characters), **and** every zero-like glyph sits where the shape
needs a digit (the digit after the prefix, or the portable suffix). If any zero-like glyph is in a letter position the token is left whole.

**Errors.** None raised. A non-string gives `("", [])`.

**Special concerns.** Pure and synchronous: no locks, files, network or database. Idempotent (the output fed back in is unchanged). The
ASCII look-alikes O/0, I/l/1, S/5, B/8 and Z/2 are never converted, so a callsign typed with a letter O is left for a person to correct.
Security: this is the defence against homoglyph callsigns; every replacement inside a callsign token, including each Greek or Cyrillic
letter, is listed in `changes` and the caller must put it in the candidate's notes. A token with no digit, such as a Cyrillic word that
looks like letters, is never callsign-shaped and is left for validation to reject. Cost is linear in the length of the text.

**See also.** Tests `[@ANCHOR: zero_sudo:test_glyph_normalizer_callsign]` in `zero_sudo/tests/test_glyph_normalizer.py`; helpers below.

### normalize_number_text `[@ANCHOR: zero_sudo:glyph_normalizer_normalize_number_text]`

**Purpose.** Turns a number written with the digits, separators and minus sign of any script into ASCII, so that a frequency or offset
on a club page (full-width `１４５．４７０`, Arabic-Indic `١٤٥٫٥`, `14Ø.5`) can be parsed by ordinary code. Used by
`repeater_page_text.py` before it reads frequencies, offsets and tones.

**Signature.** `normalize_number_text(text) -> (str, list[tuple[str, str, str]])`

| Argument | Type | Meaning |
|---|---|---|
| `text` | `str` | Page text containing numbers, one value or a line. Anything that is not a `str` is treated as empty. |

**Returns.** `(clean_text, changes)`; ASCII input comes back as the same object with `[]` at once. Stages 1 to 4 of
`normalize_callsign_text` apply. In addition the Arabic decimal separator (U+066B) and the one-dot leader (U+2024) become `.`, the Arabic
thousands separator (U+066C), Arabic comma and low quotation mark U+201A become `,`, and a minus-like sign (Unicode minus, hyphens,
dashes, full-width hyphen-minus, superscript or subscript minus) directly before a digit becomes `-`. A zero-like glyph becomes `0` only
inside a whitespace-separated token that then matches `_NUMBER_SHAPE` (optional sign, digits with `.` or `,` groups) and that has more
real digits than zero-like glyphs, so `14Ø.5` is `140.5` but `Øl 146` and `København 5` keep their letters.

**Errors.** None raised; a non-string gives `("", [])`.

**Special concerns.** Pure, synchronous, idempotent, linear, no side effects. A comma or dot is never interpreted as decimal or
thousands: `146,730` is returned as written and the caller decides. Every change is listed, as for callsigns.

**See also.** Test `[@ANCHOR: zero_sudo:test_glyph_normalizer_number]`; helpers below.

## Internals

The stages are these private helpers, each exercised through the two public functions; they take no locks and touch nothing outside
their arguments.

### _code `[@ANCHOR: zero_sudo:glyph_normalizer_code]`

`_code(text) -> str` formats the code points of `text` for the `changes` list: one `U+XXXX` (upper-case hex, at least four digits) per
character, joined by single spaces, so a two-character cluster such as an O plus a stroke overlay gives `U+004F U+0338`. `text` is any
`str`, normally the original glyph or glyph cluster. It never raises for a `str` and returns `''` for the empty string. Callers: every
stage that records a change. Test: `test_callsigns_with_odd_glyphs_are_repaired_and_every_change_is_listed`, which checks each code
starts with `U+`.

### _nfkc `[@ANCHOR: zero_sudo:glyph_normalizer_nfkc]`

`_nfkc(ch) -> str` returns `unicodedata.normalize("NFKC", ch)` for one character: the compatibility folding of full-width letters
and digits, ligatures, superscripts, circled and mathematical forms. The result may be longer than one character (a ligature) or
the same character (nothing to fold). It is applied one character at a time, never to the whole text, so that the per-character
original stays known for the `changes` list and the combining-mark stage still sees its base. A multi-character result is kept
piece by piece by `_clean_units`. No errors, no state.

### _digit_of `[@ANCHOR: zero_sudo:glyph_normalizer_digit_of]`

`_digit_of(ch) -> str | None` returns the ASCII digit a non-ASCII character stands for, or `None`. An Nd character (decimal digit of
any script) goes through `unicodedata.decimal`; a No character (for example a dingbat circled digit) through `unicodedata.digit`;
every other category, and a No character with no digit value (a fraction), gives `None`. The `ValueError` those functions raise for
a character without the value is caught and returned as `None`, so it never raises. Called by `_clean_units` for each character NFKC
left alone.

### _clean_units `[@ANCHOR: zero_sudo:glyph_normalizer_clean_units]`

`_clean_units(text, changes) -> list[[shown, original, zero_like]]` runs stages 1 to 4 over `text` and returns one three-item list per
surviving character: the text now shown, the glyph or cluster it came from, and a flag that it is a zero-like glyph. `changes` is a
list the function appends to (mutated in place). ASCII characters pass through unchanged. Cf characters are dropped, Zs, Zl and Zp
become a space, a Mn or Me mark after a digit is dropped, and after an `O` or `o` carrying a U+0335 to U+0338 overlay turns the unit
into a slashed O with the flag set, a mark after anything else is kept as its own unit. Then NFKC, then `_digit_of`, and a character in
`ZERO_LIKE` is kept with the flag set. The units are mutable lists because the callers later rewrite the first item. No errors.

### _callsign_candidate `[@ANCHOR: zero_sudo:glyph_normalizer_callsign_candidate]`

`_callsign_candidate(token_units) -> (str, list[int], list[int])` builds the text a callsign token would have if its zero-like glyphs were
`0` and its Greek and Cyrillic look-alike letters were the Latin letter, and returns the indexes of the zero-like units and of the
homoglyph units so the caller can check positions and report each one. `token_units` are units from `_clean_units`, read only. It
does not decide whether the result is a callsign: `normalize_callsign_text` matches it against `_CALLSIGN_SHAPE` and refuses a
mixed-case token (so `Bjørn` stays a word).

### _digit_positions `[@ANCHOR: zero_sudo:glyph_normalizer_digit_positions]`

`_digit_positions(match) -> set[int]` takes a successful `_CALLSIGN_SHAPE` match and returns the character indexes where the shape
requires a digit: the span of the group `d` (the digit after the prefix) and, when the callsign has a `/` or `-` suffix, the span of
group `port`, because a portable suffix such as `/Ø` is a call area digit. `normalize_callsign_text` allows a zero-like glyph to
become `0` only at these indexes, which is why `W1AØ` (a slashed O as a suffix letter) stays as typed. Pure, no errors.

## Verification

`zero_sudo/tests/test_glyph_normalizer.py` is a `BaseCase` suite (no database, no server) that calls the pure functions with the text real pages carry.

### test_callsigns_with_odd_glyphs_are_repaired_and_every_change_is_listed `[@ANCHOR: zero_sudo:test_glyph_normalizer_callsign]`

Covers `normalize_callsign_text` and all six helpers above. It checks about two dozen real or documented spellings (slashed zero, empty set, diameter sign,
theta, phi, crossed seven, dotted zero, full-width, Arabic-Indic, mathematical and circled digits, zero-width characters, a portable-suffix
zero) against their ASCII callsign, that every changed input reports at least one change whose code starts with `U+`, and the exact change
tuple for `WØAZR`. Neighbouring tests in the file check that place names and a slashed O in a letter position are left alone, that ASCII
look-alikes are never guessed, and that Greek or Cyrillic look-alikes are mapped only inside a callsign-shaped token and always listed.

### test_numbers_in_any_script_become_ascii `[@ANCHOR: zero_sudo:test_glyph_normalizer_number]`

Covers `normalize_number_text`: full-width, Arabic-Indic, Extended Arabic-Indic, Devanagari and mathematical digits, the Arabic decimal and
thousands separators, the Unicode minus and the full-width hyphen-minus, a slashed zero between digits, a crossed seven, circled and
superscript digits, zero-width characters, and that `146 MHz` and `146,730` are returned as written. Related tests in the file map every
Unicode decimal digit to its ASCII digit, check idempotence over a seeded fuzz of random code points, that ASCII passes through
untouched within half a second for a large input, and that a non-string gives `("", [])`.
