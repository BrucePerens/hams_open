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

1. A page's callsign text goes through `normalize_callsign_text()`
   `[@ANCHOR: zero_sudo:glyph_normalizer_normalize_callsign_text]`. Zero-width and other format characters are removed and every
   space separator becomes a plain space; compatibility forms (full-width, circled, superscript, mathematical digits) become
   ASCII; every decimal digit of any script becomes its ASCII digit; a combining stroke, slash or dot on a digit (a crossed seven,
   0 with a slash, a dotted zero) is removed and a letter O carrying a stroke counts as a slashed zero.
2. A zero-like glyph (O with stroke, empty set, diameter sign, theta, barred O, phi) is read as 0, and a Greek or Cyrillic
   capital identical to a Latin letter as that letter, ONLY inside a token that is then the shape of a callsign, and a zero
   only where the shape needs a digit. A Danish place name is not touched. O/0, I/l/1, S/5, B/8 and Z/2 typed in ASCII are never
   converted.
3. The call returns the list of every substitution (the glyph, its code point, what replaced it) so the caller writes it into the
   candidate's notes, and a repaired callsign always says what was repaired.
4. A number goes through `normalize_number_text()` `[@ANCHOR: zero_sudo:glyph_normalizer_normalize_number_text]`: digits of any
   script, full-width and Arabic decimal and thousands separators, and the Unicode minus become ASCII, and a zero-like glyph
   between digits is a zero. The caller decides what a comma means.

## Verification

`zero_sudo/tests/test_glyph_normalizer.py` (`[@ANCHOR: zero_sudo:test_glyph_normalizer_callsign]`,
`[@ANCHOR: zero_sudo:test_glyph_normalizer_number]`) has the real examples, every Unicode decimal digit mapped to its ASCII digit,
idempotence, ASCII passed through untouched and fast, and a fuzz over random code points.
