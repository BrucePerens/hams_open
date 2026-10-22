# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later

"""
Tests for `zero_sudo/daemon/glyph_normalizer.py`, the pure callsign / number glyph clean-up shared by the Odoo import code
and the crawl daemons. The functions are pure, so these call them with the exact text a real page carries. The callsigns
are the ones real clubs print on their own sites (W0AZR, K0ABC style) or documented, never allocated examples.
"""
import random
import time
import unicodedata

from odoo.tests.common import BaseCase, tagged

from odoo.addons.zero_sudo.daemon.glyph_normalizer import normalize_callsign_text, normalize_number_text


@tagged("post_install", "-at_install")
class TestGlyphNormalizer(BaseCase):

    # [@ANCHOR: zero_sudo:test_glyph_normalizer_callsign]
    def test_callsigns_with_odd_glyphs_are_repaired_and_every_change_is_listed(self):
        # Tests [@ANCHOR: zero_sudo:glyph_normalizer_normalize_callsign_text]
        cases = {
            "WØAZR": "W0AZR",  # slashed zero (U+00D8), the Austin ARC header
            "KØABC": "K0ABC",
            "wøazr": "w0azr",
            "W∅AZR": "W0AZR",  # empty set
            "K⌀ABC": "K0ABC",  # diameter sign
            "WΘAZR": "W0AZR",  # theta
            "WӨAZR": "W0AZR",  # Cyrillic barred O
            "WΦAZR": "W0AZR",  # phi
            "WƟAZR": "W0AZR",  # barred O
            "W0AZR": "W0AZR",  # an ordinary callsign
            "K7̶XYZ": "K7XYZ",  # a crossed seven: 7 plus U+0336
            "W0̸AZR": "W0AZR",  # 0 plus the solidus overlay
            "W0̇AZR": "W0AZR",  # a dotted zero
            "WO̸AZR": "W0AZR",  # a letter O carrying a slash is a slashed zero
            "ＷＢ４ＸＹＺ": "WB4XYZ",  # full-width letters and digit
            "W٤AZR": "W4AZR",  # Arabic-Indic digit four
            "W\U0001d7d1AZR": "W3AZR",  # mathematical bold three
            "K⑧ABC": "K8ABC",  # circled eight
            "W²ABC": "W2ABC",  # superscript two
            "W​1­AW﻿": "W1AW",  # zero-width characters inside a callsign
            "Home of WØAZR repeaters": "Home of W0AZR repeaters",  # NBSP and narrow NBSP
            "W1AW/Ø": "W1AW/0",  # a portable-suffix zero
        }
        for source, expected in cases.items():
            clean, changes = normalize_callsign_text(source)
            self.assertEqual(clean, expected, ascii(source))
            if source != "W0AZR":
                self.assertTrue(changes, ascii(source))
            for original, code, replacement in changes:
                self.assertTrue(code.startswith("U+"), changes)
                self.assertIsInstance(original, str)
                self.assertIsInstance(replacement, str)
        clean, changes = normalize_callsign_text("WØAZR")
        self.assertEqual(changes, [("Ø", "U+00D8", "0")])
        self.assertEqual(normalize_callsign_text("W0AZR"), ("W0AZR", []))

    def test_a_glyph_outside_a_callsign_or_outside_a_digit_position_is_left_for_validation_to_reject(self):
        # Tests [@ANCHOR: zero_sudo:glyph_normalizer_normalize_callsign_text]
        for word in ("Østerbro", "ØRSTA ARC", "Bjørn Hansen", "København", "Ø", "WAØZRØ",
                     "ØWAZR", "WØØZR"):
            clean, _changes = normalize_callsign_text(word)
            self.assertIn("Ø" if "Ø" in word else "ø", clean, word)
        # the digit position is the one after the prefix: a slashed O as a LETTER of the suffix is not a zero
        self.assertEqual(normalize_callsign_text("W1AØ")[0], "W1AØ")

    def test_ascii_look_alikes_are_never_guessed(self):
        # Tests [@ANCHOR: zero_sudo:glyph_normalizer_normalize_callsign_text]
        for typo in ("WSOQ", "WOAZR", "W5OQ", "WIAW", "WlAW", "KB8ABC", "W1AW", "BO4XYZ"):
            self.assertEqual(normalize_callsign_text(typo), (typo, []))

    def test_greek_and_cyrillic_look_alikes_are_mapped_only_inside_a_callsign_shaped_token_and_always_listed(self):
        # Tests [@ANCHOR: zero_sudo:glyph_normalizer_normalize_callsign_text]
        # The homoglyph attack: no digit, so not callsign-shaped, so left as the page wrote it for validation to reject.
        self.assertEqual(normalize_callsign_text("WАZR"), ("WАZR", []))
        self.assertEqual(normalize_callsign_text("АВС")[0], "АВС")
        # Inside a callsign-shaped token the table maps it, and the change says so.
        clean, changes = normalize_callsign_text("W1АВС")
        self.assertEqual(clean, "W1ABC")
        self.assertEqual([c[:2] for c in changes], [("А", "U+0410"), ("В", "U+0412"), ("С", "U+0421")])
        # Words stay words.
        self.assertEqual(normalize_callsign_text("Москва club")[0], "Москва club")

    # [@ANCHOR: zero_sudo:test_glyph_normalizer_number]
    def test_numbers_in_any_script_become_ascii(self):
        # Tests [@ANCHOR: zero_sudo:glyph_normalizer_normalize_number_text]
        cases = {
            "１４５．４７０": "145.470",  # full-width, the case from a club page
            "١٤٥": "145",  # Arabic-Indic
            "۱۴۵": "145",  # Extended Arabic-Indic
            "१४५": "145",  # Devanagari
            "\U0001d7d9\U0001d7dc\U0001d7de": "147",  # mathematical double-struck 1, 4, 6? (1 4 6 -> checked below)
            "١٤٥٫٥": "145.5",  # Arabic decimal separator
            "1٬234": "1,234",  # Arabic thousands separator
            "−٥": "-5",  # Unicode minus
            "－７．５": "-7.5",  # full-width hyphen-minus
            "14Ø.5": "140.5",  # a slashed zero between digits
            "10Θ": "100",
            "7̶.5": "7.5",  # a crossed seven
            "0̇.5": "0.5",  # a dotted zero
            "①②": "12",  # circled digits
            "1²": "12",  # superscript
            "14​6.94": "146.94",
            "146 MHz": "146 MHz",
            "146,730": "146,730",  # a separator is not interpreted here
        }
        cases["\U0001d7d9\U0001d7dc\U0001d7de"] = "146"
        for source, expected in cases.items():
            self.assertEqual(normalize_number_text(source)[0], expected, ascii(source))
        # a slashed O in a word is not a zero
        self.assertEqual(normalize_number_text("Øl 146")[0], "Øl 146")
        self.assertEqual(normalize_number_text("København 5")[0], "København 5")

    def test_every_decimal_digit_of_every_script_maps_to_its_ascii_digit(self):
        # Tests [@ANCHOR: zero_sudo:glyph_normalizer_normalize_number_text]
        # Tests [@ANCHOR: zero_sudo:glyph_normalizer_normalize_callsign_text]
        seen = 0
        for codepoint in range(0x110000):
            ch = chr(codepoint)
            if unicodedata.category(ch) != "Nd":
                continue
            seen += 1
            self.assertEqual(normalize_number_text(ch)[0], str(unicodedata.decimal(ch)), f"U+{codepoint:04X}")
            self.assertEqual(normalize_callsign_text("W" + ch + "AW")[0], "W" + str(unicodedata.decimal(ch)) + "AW", f"U+{codepoint:04X}")
        self.assertGreater(seen, 600)

    def test_idempotent_ascii_untouched_and_never_raises(self):
        # Tests [@ANCHOR: zero_sudo:glyph_normalizer_normalize_number_text]
        # Tests [@ANCHOR: zero_sudo:glyph_normalizer_normalize_callsign_text]
        ascii_text = "Home of the W1AW repeaters 146.94 MHz -0.6 PL 107.2\n" * 20000
        started = time.monotonic()
        for function in (normalize_callsign_text, normalize_number_text):
            clean, changes = function(ascii_text)
            self.assertIs(clean, ascii_text)
            self.assertEqual(changes, [])
        self.assertLess(time.monotonic() - started, 0.5)
        rng = random.Random(20261022)
        interesting = "Øø∅⌀ΘАВС̶̸̇​ ٤１①٫− .,/-W0O17"
        for _ in range(3000):
            pool = interesting if rng.random() < 0.5 else None
            text = "".join(
                rng.choice(pool) if pool else chr(rng.choice((rng.randrange(0x20, 0x300), rng.randrange(0x110000))))
                for _ in range(rng.randrange(0, 12))
            )
            for function in (normalize_callsign_text, normalize_number_text):
                clean, changes = function(text)
                self.assertEqual(function(clean)[0], clean, ascii(text))
                for original, code, replacement in changes:
                    self.assertTrue(isinstance(original, str) and code.startswith("U+") and isinstance(replacement, str))
        for bad in (None, 5, b"x", [], object()):
            self.assertEqual(normalize_callsign_text(bad), ("", []))
            self.assertEqual(normalize_number_text(bad), ("", []))
