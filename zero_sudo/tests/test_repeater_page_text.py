# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later

"""
Tests for `zero_sudo/daemon/repeater_page_text.py`, built from http://oscarmn.org/Repeater.html (candidate 3666): the repeater
is written 'K<slashed zero>HNY/R', the page also says 'EchoLink node 469520 operated by KC<slashed zero>REO', and it carries the
date '14-Jan-2021'. The functions are pure.
"""
from datetime import date

from odoo.tests.common import BaseCase, tagged

from odoo.addons.zero_sudo.daemon.repeater_page_text import (
    callsigns_in_text, node_operator_callsigns, page_date, stale_page_note, strip_repeater_designator,
)

PAGE = (
    "KØHNY/R 146.940 MHz repeater. EchoLink node 469520 operated by KCØREO. "
    "Last updated 14-Jan-2021. Contact the trustee."
)


@tagged("post_install", "-at_install")
class TestRepeaterPageText(BaseCase):

    # [@ANCHOR: zero_sudo:test_repeater_page_text_designator]
    def test_a_repeater_designator_suffix_is_stripped_from_a_callsign_shaped_token_only(self):
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_strip_repeater_designator]
        for given, expected in (
            ("K0HNY/R", ("K0HNY", "/R")), ("K0HNY-R", ("K0HNY", "-R")), ("K0HNY/RPT", ("K0HNY", "/RPT")),
            ("K0HNY RPT", ("K0HNY", " RPT")), ("w5awp-r", ("W5AWP", "-R")), ("WA9RTI/REP", ("WA9RTI", "/REP")),
        ):
            self.assertEqual(strip_repeater_designator(given), expected, given)
        for kept in ("W1AW/P", "W1AW/M", "W1AW/QRP", "W1AW/VE3", "VE3/W1AW", "K0HNY", "W1AW/0", "N/A", "REPEATER", "R", "/R", "ABC/R", ""):
            self.assertEqual(strip_repeater_designator(kept), (kept.upper(), ""), kept)
        self.assertEqual(strip_repeater_designator(None), ("", ""))

    # [@ANCHOR: zero_sudo:test_repeater_page_text_node_operators]
    def test_the_operator_of_an_echolink_allstar_or_irlp_node_is_found_even_with_a_slashed_zero(self):
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_node_operator_callsigns]
        self.assertEqual(node_operator_callsigns(PAGE), {"KC0REO"})
        for text, expected in (
            ("AllStar node 1999 is run by w1aw", {"W1AW"}),
            ("IRLP node #4321 maintained by K0ABC.", {"K0ABC"}),
            ("Node operator: N0XYZ", {"N0XYZ"}),
            ("EchoLink node 469520 sponsored by KC\u00d8REO", {"KC0REO"}),
            ("The node 469520 is linked to the W1AW repeater", set()),
            ("Trustee: K0HNY", set()),
            ("", set()),
        ):
            self.assertEqual(node_operator_callsigns(text), expected, text)
        self.assertEqual(node_operator_callsigns(None), set())

    def test_every_callsign_shaped_token_of_a_page_is_listed_with_designators_removed(self):
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_callsigns_in_text]
        self.assertEqual(callsigns_in_text(PAGE), {"K0HNY", "KC0REO"})
        self.assertEqual(callsigns_in_text("no calls here, 146.94 only"), set())
        self.assertEqual(callsigns_in_text(None), set())

    # [@ANCHOR: zero_sudo:test_repeater_page_text_stale]
    def test_a_page_that_states_its_own_old_date_gets_a_note_and_a_recent_or_undated_one_does_not(self):
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_page_date]
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_stale_page_note]
        today = date(2026, 10, 22)
        self.assertEqual(page_date(PAGE), (date(2021, 1, 14), "14-Jan-2021"))
        self.assertEqual(stale_page_note(PAGE, today), "page dated 14-Jan-2021, may be out of date")
        for text, expected in (
            ("Revised: January 3, 2019", date(2019, 1, 3)), ("Page modified 2020-05-06", date(2020, 5, 6)),
            ("as of 5/6/2022", date(2022, 5, 6)), ("Updated 3 March 2023", date(2023, 3, 3)),
            ("Last update: Mar. 3, 2024. Updated 14-Jan-2021", date(2024, 3, 3)),  # the latest stated date counts
        ):
            self.assertEqual(page_date(text)[0], expected, text)
        self.assertEqual(stale_page_note("Last updated 14-Jan-2025", today), "")  # under two years old
        self.assertEqual(stale_page_note("Last updated 22-Oct-2024", today), "")  # exactly two years
        self.assertEqual(stale_page_note("Net meets Tuesday. Copyright 2015.", today), "")  # no date the page states of itself
        self.assertEqual(stale_page_note("Updated 31-Feb-2020 and 99/99/2020", today), "")  # impossible dates are not dates
        self.assertEqual(stale_page_note(None, today), "")
