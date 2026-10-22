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
    apply_page_checks, callsign_adjacent, callsign_core, callsigns_in_text, frequency_on_page, ground_item, grounded_notes,
    header_callsigns, node_operator_callsigns, number_on_page, orbiting_repeater, page_date, placeholder_page, row_ownership,
    section_scope_for_frequency, stale_page_note, strip_repeater_designator,
)

# The cleaned text (polite_fetcher.clean_for_model: a line per block, a line per table row) of http://rasconline.net/repeaters/ (Radio Amateurs
# of Skagit County), abridged: the club's own detail blocks, then a table headed "Other Local Repeaters" with NO callsign column.
RASC = "\n".join([
    "Skip to content", "Radio Amateurs of Skagit County", "Repeaters", "Repeaters",
    "RASC VHF Repeater", "Call Sign: N7GDE", "Frequency: 145.190 MHz", "Offset: -0.600 MHz", "CTCSS Tone: 127.3 Hz",
    "The N7GDE repeater is a voice repeater and does not process any tone, DTMF, paging, or digital signals.",
    "NCRA UHF Repeater", "Call Sign: N7CRA", "Frequency: 444.625 MHz", "Offset: +5.000 MHz", "CTCSS Tone: 103.5 Hz",
    "The N7CRA repeater is operated by the North Cascade Repeater Association. It is located on Cultus Mountain at an altitude of 3,900 ft.",
    "Digital (DMR) Repeater", "*** This repeater is currently not in service ***", "Call Sign: KF7CFR", "Frequency: 441.950 MHz",
    "Talkgroup Name | Talkgroup # | Time Slot | Notes *", "WASH 1 | 3153 | 1 | FT / BM / PNWD",
    "Other Local Repeaters", "Location | Frequency | Offset / CTCSS",
    "Cultus Mountain | 53.590 MHz | -1.700 MHz / 100.0 Hz", "Oak Harbor | 146.8625 MHz | -0.600 MHz / 114.8 Hz / NFM",
    "Mount Constitution | 146.740 MHz | -0.600 MHz / 103.5 Hz", "Mount Constitution | 443.450 MHz | +5.000 MHz / 103.5 Hz",
    "Cultus Mountain | 927.550 MHz | -25.000 MHz / 114.8 Hz",
    "Radio Amateurs of Skagit County", "Copyright \u00a9 2026 \u2013 Radio Amateurs of Skagit County",
])

# http://ac6ee.org/repeaters.html (Tehachapi Amateur Radio Association, club callsign AC6EE), abridged: ONE table of repeaters with five
# licensees, a club APRS digipeater, other organisations' linked systems, and a list of repeaters elsewhere.
TARA = "\n".join([
    "CONDITIONS", "TARA AC6EE \u00b7 146.700(\u2212) 123.0 PL", "Tehachapi Amateur Radio Assoc. AC6EE \u00b7 ARRL AFFILIATED",
    "Home Membership Repeaters Nets Get Licensed Newsletters", "Repeater Information",
    "Local repeaters serving Tehachapi and Bear Valley Springs", "Tehachapi Area Repeaters", "Frequency | Tone | Call Sign | Notes",
    "146.700 (\u2212) | 123.0 | W6SLZ | BVS \u00b7 Open \u00b7 Primary EmComm",
    "440.625 (+) | 100.0 | W6SLZ | BVS \u00b7 Open \u00b7 Win System",
    "144.390 | None | AC6EE-3 | APRS Digipeater",
    "145.150 (\u2212) | 100.0 | W6LIE | Kern County Central Valley ARC",
    "442.925 (+) | 141.3 | KI6HHU | Cummings Mtn \u00b7 KERN System",
    "444.425 (+) | 100.0 | KG6KKV | ARRG Linked Repeater",
    "447.120 (\u2212) | 67.0 | KR6DK | KR6DK Bilingual Repeater Network",
    "KERN Linked Repeater System", "442.925(+) PL 141.3 \u2014 Cummings Mountain 7,800 ft",
    "DMR Repeaters", "Callsign | Frequency | CC | Notes", "K6GTA | 442.225 +5 | 1 | Brandmeister \u00b7 Bear Valley Springs",
    "ARRG Linked Repeater System", "444.425(+) PL 100 at Cummings Mountain \u2014 all linked full-time.",
    "Useful Antelope Valley Repeaters", "146.730(\u2212) PL 100 \u2014 Hauser Peak, Palmdale",
    "Navigate", "Tehachapi Amateur Radio Association (AC6EE) \u2014 serving the Tehachapi Mountain communities.",
    "\u00a9 2026 Tehachapi Amateur Radio Association \u00b7 AC6EE", "73 de AC6EE",
])

# http://9y4.tt/repeaters/: nothing but the unedited WordPress sample page.
SAMPLE_PAGE = "\n".join([
    "Skip to content", "TTARS", "About", "Repeaters", "Repeaters in the Caribbean",
    "This is an example page. It\u2019s different from a blog post because it will stay in one place and will show up in your site navigation.",
    "Hi there! I\u2019m a bike messenger by day, aspiring actor by night, and this is my website. I live in Los Angeles.",
    "\u2026or something like this:",
    "The XYZ Doohickey Company was founded in 1971, and has been providing quality doohickeys to the public ever since.",
    "As a new WordPress user, you should go to your dashboard to delete this page and create new pages for your content. Have fun!",
    "Copyright \u00a9 2026 TTARS | Powered by Astra WordPress Theme",
])

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


@tagged("post_install", "-at_install")
class TestRepeaterPageStructure(BaseCase):
    # Tests [@ANCHOR: zero_sudo:repeater_page_text_section_scope_for_frequency]

    # [@ANCHOR: zero_sudo:test_repeater_page_text_sections]
    def test_rasc_the_other_local_repeaters_table_is_tagged_other_and_the_detail_blocks_club(self):
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_append_note]
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_clean]
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_frequencies]
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_heading_kind]
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_item_frequency]
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_month]
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_near]
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_occurrences]
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_own_span]
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_page_numbers]
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_structure]
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_title]
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_title_words]
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_to_date]
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_section_scope_for_frequency]
        for freq in (53.59, 146.8625, 146.74, 443.45, 927.55):
            self.assertEqual(section_scope_for_frequency(RASC, freq), ("other", "Other Local Repeaters"), freq)
        for freq, heading in ((145.19, "RASC VHF Repeater"), (444.625, "NCRA UHF Repeater"), (441.95, "Digital (DMR) Repeater")):
            self.assertEqual(section_scope_for_frequency(RASC, freq), ("club", heading), freq)
        self.assertEqual(section_scope_for_frequency(RASC, 147.93), (None, ""))  # not on the page
        self.assertEqual(section_scope_for_frequency(None, 145.19), (None, ""))

    def test_headings_that_set_repeaters_apart_are_recognised_in_several_wordings_and_languages(self):
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_section_scope_for_frequency]
        for heading in ("Other Repeaters", "Local and Regional Repeaters", "Nearby Repeaters", "Neighboring Repeaters", "Surrounding Repeaters",
                        "Other Clubs' Repeaters", "Other local repeaters:", "Andere Relais", "Otros repetidores", "Autres relais",
                        "Useful Antelope Valley Repeaters", "KERN Linked Repeater System"):
            text = f"Our Repeaters\nCall Sign: W1AW\nFrequency: 146.940 MHz\n{heading}\nSite | 147.120 MHz | +0.6"
            self.assertEqual(section_scope_for_frequency(text, 147.12)[0], "other", heading)
            self.assertEqual(section_scope_for_frequency(text, 146.94)[0], "club", heading)

    def test_a_page_with_no_sections_stays_unknown_and_a_weak_heading_needs_a_club_section_beside_it(self):
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_section_scope_for_frequency]
        plain = "Our club\nWe meet Tuesday.\n146.940 MHz PL 100\n147.120 MHz PL 100"
        self.assertEqual(section_scope_for_frequency(plain, 146.94), (None, ""))
        only_area = "Area Repeaters\nCall | Rx | Tx\nW5AWP | 146.375 | 146.975"
        self.assertEqual(section_scope_for_frequency(only_area, 146.975), (None, ""))  # the club's only list is not another club's
        beside_club = "Club Repeaters\n146.940 MHz\nArea Repeaters\n147.120 MHz"
        self.assertEqual(section_scope_for_frequency(beside_club, 147.12)[0], "other")
        self.assertEqual(section_scope_for_frequency(beside_club, 146.94)[0], "club")

    def test_the_more_specific_place_wins_and_a_tie_is_left_to_the_model(self):
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_section_scope_for_frequency]
        both = "Our Repeaters\nCall Sign: W1AW\nFrequency: 146.940 MHz\nOther Repeaters\nSite | 146.940 MHz | -0.6\nSite | 147.120 MHz | +0.6"
        self.assertEqual(section_scope_for_frequency(both, 146.94), (None, ""))  # a detail block and a table row: a tie
        prose = "Our Repeaters\nCall Sign: W1AW\nFrequency: 146.940 MHz\nOther Repeaters\nThe pair 146.940 is linked.\n"
        self.assertEqual(section_scope_for_frequency(prose, 146.94)[0], "club")  # the detail block beats a prose mention

    def test_apply_page_checks_never_lets_the_model_loosen_a_deterministic_other_but_may_tighten(self):
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_apply_page_checks]
        items = apply_page_checks([
            {"name": "Cultus", "output_frequency": 53.59, "listing_scope": "club", "callsign": "N7CRA"},
            {"name": "NCRA", "output_frequency": 444.625, "listing_scope": "other", "callsign": "N7CRA"},
            {"name": "GDE", "output_frequency": 145.19, "callsign": "N7GDE"}, "stray"], RASC)
        self.assertEqual(items[0]["listing_scope"], "other")  # the model said club; the page says otherwise
        self.assertEqual(items[0]["listing_section"], "Other Local Repeaters")
        self.assertEqual(items[1]["listing_scope"], "other")  # the model may make it stricter
        self.assertEqual(items[2]["listing_scope"], "club")
        self.assertEqual(items[3], "stray")
        self.assertEqual(apply_page_checks(["x"], None), ["x"])

    # [@ANCHOR: zero_sudo:test_repeater_page_text_adjacency]
    def test_a_callsign_borrowed_from_another_block_is_removed_with_a_note(self):
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_callsign_adjacent]
        # Candidate 3873: 'Cultus Mountain 53.590' accepted with N7CRA, the callsign of the block above it.
        self.assertIs(callsign_adjacent(RASC, "N7CRA", 53.59), False)
        self.assertIs(callsign_adjacent(RASC, "N7CRA", 444.625), True)
        self.assertIs(callsign_adjacent(RASC, "N7GDE", 444.625), False)
        self.assertIs(callsign_adjacent(RASC, "n7gde", 145.19), True)
        self.assertIsNone(callsign_adjacent(RASC, "N7CRA", 147.93))  # frequency not on the page: nothing can be said
        self.assertIsNone(callsign_adjacent(RASC, "W9ZZZ", 53.59))  # callsign nowhere on the page
        checked = apply_page_checks([{"name": "Cultus Mountain 53.590", "output_frequency": 53.59, "callsign": "N7CRA"}], RASC)[0]
        self.assertNotIn("callsign", checked)
        self.assertIn("callsign from another section removed", checked["notes"])

    def test_a_callsign_in_the_section_heading_or_the_page_header_belongs_to_its_rows(self):
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_callsign_adjacent]
        home = "Home of the W\u00d8AZR Repeaters\nLocal 145.47 MHz\nCounty 146.73 MHz\nLink to 73 445.850"
        self.assertIs(callsign_adjacent(home, "W0AZR", 146.73), True)  # the heading names it, whatever the glyph
        header = "TI0RC\nRed Nacional\nSite | 146.940\nSite | 147.060\nEstacion oficial de la red: TI0RC"
        self.assertIs(callsign_adjacent(header, "TI0RC", 146.94), True)  # the network's own callsign, in the header and footer

    # [@ANCHOR: zero_sudo:test_repeater_page_text_ownership]
    def test_tara_only_the_clubs_own_callsign_is_club_evidence_everything_else_is_unclear_or_other(self):
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_row_ownership]
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_apply_page_checks]
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_header_callsigns]
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_callsign_core]
        self.assertEqual(header_callsigns(TARA), {"AC6EE"})
        self.assertEqual(callsign_core("AC6EE-3"), "AC6EE")
        self.assertEqual(callsign_core("ac6ee/R"), "AC6EE")
        self.assertEqual(callsign_core("W1AW/P"), "W1AW/P")
        self.assertEqual(callsign_core("AC6EE-16"), "AC6EE-16")  # not an SSID
        rows = [("W6SLZ", 146.7), ("W6SLZ", 440.625), ("AC6EE-3", 144.39), ("W6LIE", 145.15), ("KI6HHU", 442.925), ("KG6KKV", 444.425),
                ("KR6DK", 447.12), ("K6GTA", 442.225)]
        got = {(i["callsign"], i["output_frequency"]): i["listing_ownership"]
               for i in apply_page_checks([{"name": c, "callsign": c, "output_frequency": f} for c, f in rows], TARA)}
        self.assertEqual(got[("AC6EE-3", 144.39)], "club")
        for key in rows:
            if key != ("AC6EE-3", 144.39):
                self.assertEqual(got[key], "unclear", key)
        self.assertEqual(row_ownership(TARA, 146.73, None), "other")  # the Antelope Valley list
        self.assertEqual(row_ownership(TARA, 146.7, "W6SLZ"), None)

    def test_a_list_with_one_licensee_and_no_other_evidence_is_unspecified_not_unclear(self):
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_apply_page_checks]
        text = "Our club\nRepeaters\nSite | 146.940 | K9HGX\nSite | 147.120 | K9HGX"  # a short page, callsign only in rows
        got = apply_page_checks([{"callsign": "K9HGX", "output_frequency": 146.94}, {"callsign": "K9HGX", "output_frequency": 147.12}], text)
        self.assertEqual([i["listing_ownership"] for i in got], ["unspecified", "unspecified"])
        stated = "Repeaters\n146.940 MHz W1AAA - we operate this one\nSite | 147.120 | W1BBB | operated by Someone Else"
        got = apply_page_checks([{"callsign": "W1AAA", "output_frequency": 146.94}, {"callsign": "W1BBB", "output_frequency": 147.12}], stated)
        self.assertEqual([i["listing_ownership"] for i in got], ["club", "other"])

    # [@ANCHOR: zero_sudo:test_repeater_page_text_grounding]
    def test_a_repeater_whose_frequency_is_not_on_the_page_is_not_on_page(self):
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_ground_item]
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_frequency_on_page]
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_number_on_page]
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_grounded_notes]
        # 9y4.tt: three repeaters invented from the page title of the unedited WordPress sample page.
        for freq in (147.93, 146.94, 147.76):
            invented, problems = ground_item(
                {"name": "Northern Range", "output_frequency": freq, "notes": "Listed as available for emergency use by the U.S. Navy."}, SAMPLE_PAGE)
            self.assertEqual(invented["not_on_page"], "the extracted frequency is not stated on the page (possibly invented by the model)")
            self.assertNotIn("notes", invented)
            self.assertTrue(problems)
        stated = "Output 147.930 MHz, input 147.330, PL 107.2. Call W1AW. 50 W."
        for freq in (147.93, 147.930, 147.9300, 147.9300001):
            self.assertTrue(frequency_on_page(stated, freq), freq)
        self.assertTrue(frequency_on_page("Freq 147930 kHz", 147.93))
        self.assertTrue(frequency_on_page("Freq 147,93 MHz", 147.93))
        self.assertTrue(frequency_on_page("Freq 147.930.000 Hz 146,520,000", 146.52))
        self.assertTrue(frequency_on_page("\uff11\uff14\uff17\uff0e\uff19\uff13", 147.93))
        self.assertFalse(frequency_on_page(stated, 147.94))
        self.assertFalse(frequency_on_page(stated, True))
        self.assertTrue(number_on_page(stated, 107.2))
        self.assertFalse(number_on_page(stated, 110.9))

    def test_a_grounded_repeater_keeps_what_the_page_states_and_loses_what_it_does_not(self):
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_ground_item]
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_grounded_notes]
        page = "W\u00d8AZR repeater. Output 146.940 MHz, PL 107.2. Net on Tuesday. 50 W output."
        item, problems = ground_item({
            "callsign": "W0AZR", "output_frequency": 146.94, "input_frequency": 146.34, "tone_uplink": 107.2, "tone_downlink": 88.5,
            "notes": "Net on Tuesday. 50 W output. Coverage 120 miles. Listed by the Red Cross."}, page)
        self.assertEqual(item["callsign"], "W0AZR")
        self.assertNotIn("not_on_page", item)
        self.assertNotIn("input_frequency", item)  # never stated
        self.assertNotIn("tone_downlink", item)
        self.assertEqual(item["tone_uplink"], 107.2)
        self.assertEqual(item["notes"], "Net on Tuesday. 50 W output.")
        self.assertTrue(problems)
        gone, _ = ground_item({"callsign": "W9XYZ", "output_frequency": 146.94}, page)
        self.assertNotIn("callsign", gone)  # a callsign the page never states
        self.assertEqual(grounded_notes("Nothing special here.", page), ("Nothing special here.", []))
        self.assertEqual(ground_item("stray", page), ("stray", []))

    # [@ANCHOR: zero_sudo:test_repeater_page_text_placeholder]
    def test_an_unedited_default_page_is_a_placeholder_and_a_real_page_that_mentions_one_is_not(self):
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_placeholder_page]
        self.assertEqual(placeholder_page(SAMPLE_PAGE), "This is an example page")
        for text, phrase in (
            ("Welcome to nginx!\nIf you see this page, the nginx web server is successfully installed and working. Further configuration is required.", "Welcome to nginx"),
            ("Apache2 Ubuntu Default Page\nIt works!\nThis is the default welcome page used to test the correct operation of the Apache2 server.", "Apache2 Ubuntu Default Page"),
            ("Home\nHello world! Welcome to WordPress. This is your first post. Edit or delete it, then start writing!", "Hello world! Welcome to WordPress"),
            ("Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.", "Lorem ipsum"),
            ("Coming soon", "Coming soon"), ("Website coming soon", "website coming soon"), ("This domain is for sale", "this domain is for sale"),
        ):
            self.assertEqual(placeholder_page(text).lower(), phrase.lower(), text)
        real = (RASC + "\nThis page is under construction while we add photos of the other repeater sites we help to maintain on the mountain.")
        self.assertEqual(placeholder_page(real), "")  # a real page that merely mentions the phrase
        self.assertEqual(placeholder_page("Under construction. Our repeater is on 146.940 MHz, PL 100, trustee W1AW."), "")  # states a frequency
        self.assertEqual(placeholder_page("The club meets on the first Tuesday of each month at the fire hall. Dues are twenty dollars a year, "
                                          "and new members are always welcome. Coming soon: a new DMR repeater, details to follow in the newsletter."), "")
        self.assertEqual(placeholder_page(""), "")
        self.assertEqual(placeholder_page(None), "")


@tagged("post_install", "-at_install")
class TestOrbitingRepeater(BaseCase):

    # [@ANCHOR: zero_sudo:test_repeater_page_text_orbiting]
    def test_the_iss_and_amateur_satellites_are_orbiting_repeaters_and_terrestrial_ones_are_not(self):
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_orbiting_repeater]
        positives = (
            # bdxc.org (candidate 796) and crecj.org (candidate 805)
            dict(name="ARISS Repeater", notes="ISS repeater; uplink 145.990 MHz (PL 67.0 Hz), downlink 437.800 MHz", url="https://bdxc.org/"),
            dict(name="Repetidor de la ISS (ARISS)", notes="Repetidor de la Estaci\u00f3n Espacial Internacional: subida 145.990 FM CTCSS 67, bajada 437.800",
                 url="https://crecj.org/repetidor-de-la-iss-activo/"),
            dict(name="Repeater", url="https://crecj.org/repetidor-de-la-iss-activo/"),  # the address slug names it
            dict(name="Repeater", callsign="NA1SS"), dict(name="x", callsign="rs0iss"), dict(name="x", callsign="OR4ISS"),
            dict(name="x", callsign="DP0ISS"), dict(name="x", callsign="IR0ISS"),
            dict(name="ISS cross-band repeater"), dict(name="International Space Station repeater"), dict(name="space station"),
            dict(name="Estaci\u00f3n Espacial"), dict(name="Station spatiale ISS"), dict(name="Raumstation ISS"), dict(name="Esta\u00e7\u00e3o espacial"),
            dict(name="Stazione spaziale"), dict(name="AO-91"), dict(name="SO-50 FM"), dict(name="FO-29"), dict(name="RS-44"), dict(name="XW-2A"),
            dict(name="CAS-4A"), dict(name="LO-87"), dict(name="EO-88"), dict(name="QO-100 transponder"), dict(name="Es'hail-2"), dict(name="Tianhe"),
            dict(name="UKube-1"), dict(name="LilacSat-2"), dict(name="CubeSat"), dict(name="OSCAR 7"), dict(name="OSCAR-51"), dict(name="Oscar 10 linear"),
            dict(name="Satellite uplink 145.9 downlink 435.1"), dict(name="x", section="Satellites: ARISS"), dict(name="x", section="Amateur satellite transponders"),
            dict(name="Repeater", notes="Satellite downlink on 435.800, uplink 145.900 (AO-73)"),
            dict(name="x", notes="Cross-band repeater aboard the ISS: uplink 145.990 downlink 437.800"),
            dict(name="Cross-band", notes="Uplink 145.990 FM, downlink 437.800 FM, PL 67", frequencies=(145.99, 437.8)),
        )
        for case in positives:
            self.assertTrue(orbiting_repeater(**case), case)
        negatives = (
            # oscarmn.org: OSCAR is the Owatonna Steele County Amateur Radio club, 145.490 MHz is a terrestrial repeater (K0HNY)
            dict(name="OSCAR 145.490", callsign="K0HNY", url="http://oscarmn.org/Repeater.html"), dict(name="OSCAR 145.49"),
            dict(name="OSCAR repeater"), dict(name="OSCAR 146,94"), dict(name="OSCAR 444"),
            # W3BAB Towson ATV repeater (candidate 3442): the notes only mention 'satellite' in a video format
            dict(name="W3BAB ATV", notes="Second output FM satellite format video, PL 107.2"),
            dict(name="Towson ATV", notes="Satellite receiver feeds the second output"),
            # ISS inside a longer word, a club that does ARISS contacts, a place called Satellite Beach
            dict(name="Mississippi River 146.94"), dict(name="Hiss Mountain"), dict(name="MISSION repeater"), dict(name="Wissahickon"), dict(name="Dissolve"),
            dict(name="Valley 146.94", notes="The club also makes ARISS contacts with schools and has a Field Day satellite station."),
            dict(name="Satellite Beach 146.76"), dict(name="Hill 147.12", notes="Club meets at the Space Needle."),
            dict(name="Valley repeater", url="https://club.example/repeaters/valley/"), dict(name="Valley repeater", url="https://club.example/"),
            dict(name="Valley", callsign="W1AW"), dict(name="Valley", callsign="K0HNY/R"), dict(name="x", section="Our Repeaters"),
            dict(name="Mount Isis 146.94"), dict(name="Mount Elbert RS 4"), dict(name="AO 91 Road"), dict(name="Station 7"),
            dict(), dict(name=None, notes=None, section=None, url=None, callsign=None),
        )
        for case in negatives:
            self.assertEqual(orbiting_repeater(**case), "", case)

    def test_the_matched_text_is_returned_and_hostile_input_never_raises(self):
        # Tests [@ANCHOR: zero_sudo:repeater_page_text_orbiting_repeater]
        self.assertEqual(orbiting_repeater(name="ARISS Repeater").lower(), "ariss")
        self.assertEqual(orbiting_repeater(name="OSCAR 7"), "OSCAR 7")
        self.assertEqual(orbiting_repeater(callsign="NA1SS"), "NA1SS")
        for bad in (5, b"x", [], object()):
            self.assertEqual(orbiting_repeater(name=bad, notes=bad, section=bad, url=bad, callsign=bad), "")
