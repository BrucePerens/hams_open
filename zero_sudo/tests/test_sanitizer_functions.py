# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. SPDX-License-Identifier: AGPL-3.0-or-later
"""Direct tests of the helper functions inside `css_sanitizer.py` and `svg_sanitizer.py`.

The whole-filter tests (`test_css_sanitizer.py`, `test_svg_sanitizer.py`) feed hostile and honest documents through the
public entry points. These call each helper itself with a small input and check what it returns.
"""
from lxml import etree
import lxml.html

from odoo.tests.common import BaseCase, tagged
from odoo.tools import mail

from odoo.addons.zero_sudo import css_sanitizer as css
from odoo.addons.zero_sudo import svg_sanitizer as svg


@tagged("post_install", "-at_install")
class TestCssSanitizerFunctions(BaseCase):

    def test_01_scan_stops_only_outside_strings_and_parentheses(self):
        # Tests [@ANCHOR: zero_sudo:css_scan]
        self.assertEqual(css._scan("a(b;c);d", 0, ";"), 6)
        self.assertEqual(css._scan('a";";b', 0, ";"), 4)
        self.assertEqual(css._scan("abc", 0, ";"), 3)

    def test_02_matching_brace_skips_nested_braces_and_strings(self):
        # Tests [@ANCHOR: zero_sudo:css_matching_brace]
        self.assertEqual(css._matching_brace("{a{b}c}x", 0), 6)
        self.assertEqual(css._matching_brace('{"}"}', 0), 4)
        self.assertIsNone(css._matching_brace("{a", 0))

    def test_03_parse_blocks_splits_rules_and_statements(self):
        # Tests [@ANCHOR: zero_sudo:css_parse_blocks]
        self.assertEqual(
            css._parse_blocks("a{b:c} @import 'x'; d{e}"),
            [("a", "b:c"), ("@import 'x'", None), ("d", "e")],
        )
        self.assertEqual(css._parse_blocks("a{b"), [("a", None)])
        self.assertEqual(css._parse_blocks("zzz"), [("zzz", None)])

    def test_04_clean_value_refuses_what_fetches_or_runs(self):
        # Tests [@ANCHOR: zero_sudo:css_clean_value]
        self.assertEqual(css._clean_value("color", "  red  "), "red")
        self.assertEqual(css._clean_value("color", "red !IMPORTANT"), "red !important")
        self.assertEqual(css._clean_value("background", "url(#g)"), "url(#g)")
        self.assertIsNone(css._clean_value("background", "url(https://evil.example/a)"))
        self.assertIsNone(css._clean_value("position", "fixed"))
        self.assertEqual(css._clean_value("position", "relative"), "relative")
        self.assertIsNone(css._clean_value("color", ""))

    def test_05_clean_declarations_keeps_the_good_ones(self):
        # Tests [@ANCHOR: zero_sudo:css_clean_declarations]
        self.assertEqual(css._clean_declarations("COLOR: red; background:url(http://x); bad"), ("color:red", True))
        self.assertEqual(css._clean_declarations("color:red;margin:0"), ("color:red;margin:0", False))
        self.assertEqual(css._clean_declarations("behavior:x"), ("", True))

    # Tests [@ANCHOR: zero_sudo_css_sanitizer_counter]
    def test_06_clean_rules_drops_what_is_not_allowed_and_keeps_the_rest(self):
        # Tests [@ANCHOR: zero_sudo:css_clean_rules]
        text, dropped = css._clean_rules("a{color:red} @import 'x'; b{position:fixed}", 0, css._Counter())
        self.assertEqual((text, dropped), ("a{color:red}", True))
        text, dropped = css._clean_rules("@media screen{a{color:red}}", 0, css._Counter())
        self.assertEqual((text, dropped), ("@media screen{a{color:red}}", False))

    def test_07_sanitize_stylesheet_rewrites_the_sheet(self):
        # Tests [@ANCHOR: zero_sudo:css_sanitize_stylesheet]
        self.assertEqual(css.sanitize_stylesheet("a { color : red } /* c */"), ("a{color:red}", False))
        self.assertEqual(css.sanitize_stylesheet("a{background:url(http://x)}"), ("", True))
        self.assertEqual(css.sanitize_stylesheet("  "), ("", False))


@tagged("post_install", "-at_install")
class TestSvgSanitizerFunctions(BaseCase):

    def _svg(self, text):
        return etree.fromstring(text)

    # Tests [@ANCHOR: zero_sudo_svg_sanitizer_call_totals]
    def test_01_call_totals_and_budget_start_at_zero(self):
        # Tests [@ANCHOR: zero_sudo:svg_call_totals_init]
        # Tests [@ANCHOR: zero_sudo:svg_budget_init]
        totals = svg._CallTotals()
        self.assertEqual((totals.blocks, totals.size), (0, 0))
        budget = svg._Budget(totals)
        self.assertEqual((budget.elements, budget.size), (0, 0))
        self.assertIs(budget.totals, totals)
        self.assertIsNone(svg._Budget().totals)

    # Tests [@ANCHOR: zero_sudo_svg_sanitizer_rejected]
    def test_02_budget_element_counts_up_to_the_cap(self):
        # Tests [@ANCHOR: zero_sudo:svg_budget_element]
        budget = svg._Budget()
        for _ in range(svg.MAX_SVG_ELEMENTS):
            budget.element()
        with self.assertRaises(svg._Rejected):
            budget.element()

    def test_03_budget_spend_caps_the_block_and_the_call(self):
        # Tests [@ANCHOR: zero_sudo:svg_budget_spend]
        budget = svg._Budget()
        budget.spend(svg.MAX_SVG_BYTES)
        with self.assertRaises(svg._Rejected):
            budget.spend(1)
        totals = svg._CallTotals()
        totals.size = svg.MAX_TOTAL_SVG_BYTES - 1
        shared = svg._Budget(totals)
        shared.spend(1)
        self.assertEqual(totals.size, svg.MAX_TOTAL_SVG_BYTES)
        with self.assertRaises(svg._Rejected):
            shared.spend(1)

    def test_04_check_enum_and_check_regex_build_validators(self):
        # Tests [@ANCHOR: zero_sudo:svg_check_enum]
        # Tests [@ANCHOR: zero_sudo:svg_check_regex]
        check = svg._check_enum("fill-rule")
        self.assertEqual(check("evenodd"), "evenodd")
        self.assertIsNone(check("bogus"))
        number = svg._check_regex(svg._RE_NUMBER)
        self.assertEqual(number(" 12 "), "12")
        self.assertIsNone(number("x"))
        self.assertIsNone(number("1" * (svg.SCALAR_LIMIT + 1)))

    def test_05_paint_and_url_ref_keep_only_same_document_fragments(self):
        # Tests [@ANCHOR: zero_sudo:svg_paint]
        # Tests [@ANCHOR: zero_sudo:svg_url_ref]
        self.assertEqual(svg._paint("red"), "red")
        self.assertEqual(svg._paint("url( #g ) red"), "url(#g) red")
        self.assertIsNone(svg._paint("url(http://x/y)"))
        self.assertEqual(svg._url_ref("none"), "none")
        self.assertEqual(svg._url_ref("url( #a )"), "url(#a)")
        self.assertIsNone(svg._url_ref("url(http://x/y)"))

    def test_06_viewbox_needs_four_numbers(self):
        # Tests [@ANCHOR: zero_sudo:svg_viewbox]
        self.assertEqual(svg._viewbox("0 0 10,20"), "0 0 10 20")
        self.assertIsNone(svg._viewbox("1 2 3"))
        self.assertIsNone(svg._viewbox("0 0 a b"))

    def test_07_safe_id_refuses_clobbering_names(self):
        # Tests [@ANCHOR: zero_sudo:svg_safe_id]
        self.assertEqual(svg._safe_id("a1"), "a1")
        self.assertIsNone(svg._safe_id("cookie"))
        self.assertIsNone(svg._safe_id("__proto"))
        self.assertIsNone(svg._safe_id("1a"))

    def test_08_aria_value_refuses_markup_characters(self):
        # Tests [@ANCHOR: zero_sudo:svg_aria]
        self.assertEqual(svg._aria("A diagram"), "A diagram")
        self.assertIsNone(svg._aria("a<b"))

    def test_09_small_margin_and_svg_size_have_caps(self):
        # Tests [@ANCHOR: zero_sudo:svg_small_margin]
        # Tests [@ANCHOR: zero_sudo:svg_size]
        self.assertEqual(svg._small_margin("5px"), "5px")
        self.assertEqual(svg._small_margin("9em"), "9em")
        self.assertIsNone(svg._small_margin("10em"))
        self.assertIsNone(svg._small_margin("100px"))
        self.assertIsNone(svg._small_margin("-1px"))
        self.assertEqual(svg._svg_size("auto"), "auto")
        self.assertEqual(svg._svg_size("4000px"), "4000px")
        self.assertIsNone(svg._svg_size("4001px"))
        self.assertIsNone(svg._svg_size("101%"))
        self.assertIsNone(svg._svg_size("251em"))

    def test_10_clean_style_keeps_allowlisted_declarations(self):
        # Tests [@ANCHOR: zero_sudo:svg_clean_style]
        self.assertEqual(svg._clean_style("fill:red;position:fixed;margin-top:5px"), "fill:red;margin-top:5px")
        self.assertIsNone(svg._clean_style("fill:red /* c */"))
        self.assertIsNone(svg._clean_style("position:fixed"))

    def test_11_clean_attribute_returns_canonical_name_and_value(self):
        # Tests [@ANCHOR: zero_sudo:svg_clean_attribute]
        self.assertEqual(svg._clean_attribute("FILL", " red "), ("fill", "red"))
        self.assertEqual(svg._clean_attribute("viewbox", "0 0 1 2"), ("viewBox", "0 0 1 2"))
        self.assertEqual(svg._clean_attribute("aria-label", "hi"), ("aria-label", "hi"))
        self.assertEqual(svg._clean_attribute("style", "fill:red"), ("style", "fill:red"))
        self.assertIsNone(svg._clean_attribute("aria-owns", "x"))
        self.assertIsNone(svg._clean_attribute("onclick", "x"))
        self.assertIsNone(svg._clean_attribute("fill", "javascript:x"))

    def test_12_clean_text_and_join_text_normalise_whitespace(self):
        # Tests [@ANCHOR: zero_sudo:svg_clean_text]
        # Tests [@ANCHOR: zero_sudo:svg_join_text]
        budget = svg._Budget()
        self.assertIsNone(svg._clean_text(None, True, budget))
        self.assertEqual(svg._clean_text("a\x01b", True, budget), "ab")
        self.assertEqual(svg._clean_text("  ", False, budget), "\n")
        self.assertIsNone(svg._clean_text("x", False, budget))
        self.assertEqual(svg._join_text("a", "b", True), "ab")
        self.assertEqual(svg._join_text(None, "  ", False), "\n")

    def test_13_svg_local_name_accepts_bare_and_svg_namespace_names(self):
        # Tests [@ANCHOR: zero_sudo:svg_local_name]
        self.assertEqual(svg.svg_local_name("SVG"), "svg")
        self.assertEqual(svg.svg_local_name("{http://www.w3.org/2000/svg}Rect"), "rect")
        self.assertIsNone(svg.svg_local_name("{http://www.w3.org/1999/xhtml}script"))
        self.assertIsNone(svg.svg_local_name("svg:script"))
        self.assertIsNone(svg.svg_local_name(None))

    def test_14_build_and_sanitize_element_rebuild_from_the_allowlist(self):
        # Tests [@ANCHOR: zero_sudo:svg_build]
        # Tests [@ANCHOR: zero_sudo:svg_sanitize_element]
        source = self._svg('<svg><g><path d="M0 0L1 1" onclick="x"/></g><text>a<script/>b</text></svg>')
        built = svg._build(source, 1, svg._Budget())
        text = etree.tostring(built).decode()
        self.assertIn('<path d="M0 0L1 1"/>', text)
        self.assertNotIn("onclick", text)
        self.assertNotIn("script", text)
        self.assertEqual(built.find("text").text, "ab")
        with self.assertRaises(svg._Rejected):
            svg._build(source, svg.MAX_SVG_DEPTH + 1, svg._Budget())
        clean = svg.sanitize_svg_element(source)
        self.assertEqual(clean.get("xmlns"), svg.SVG_NS)
        self.assertIsNone(svg.sanitize_svg_element(self._svg("<div/>")))
        totals = svg._CallTotals()
        totals.blocks = svg.MAX_TOTAL_SVG_BLOCKS
        self.assertIsNone(svg.sanitize_svg_element(source, _totals=totals))

    def test_15_detach_top_level_svgs_and_reattach_them(self):
        # Tests [@ANCHOR: zero_sudo:svg_top_level_svgs]
        # Tests [@ANCHOR: zero_sudo:svg_detach_svgs]
        # Tests [@ANCHOR: zero_sudo:svg_reattach_svgs]
        # Tests [@ANCHOR: zero_sudo:svg_clean_document]
        doc = lxml.html.fromstring('<div>a<svg><svg></svg></svg><p>x</p><svg onload="x"></svg>b</div>')
        self.assertEqual(len(svg._top_level_svgs(doc)), 2)
        stash = svg._detach_svgs(doc, "n0nce", ())
        self.assertEqual(sorted(stash), [0, 1])
        self.assertEqual(list(doc.iter("svg")), [])
        svg._reattach_svgs(doc, stash, "n0nce")
        self.assertEqual(len(list(doc.iter("svg"))), 3)  # the nested one is rebuilt with its parent
        self.assertNotIn("onload", etree.tostring(doc).decode())
        calls = []
        whole = lxml.html.fromstring('<div>a<svg><rect width="5"/></svg>b</div>')
        svg.clean_document(whole, lambda tree: calls.append(tree))
        self.assertEqual(calls, [whole])
        self.assertIn("<rect", etree.tostring(whole).decode())
        plain = lxml.html.fromstring("<div>a</div>")
        self.assertEqual(svg.clean_document(plain, lambda tree: "ran"), "ran")

    def test_16_a_block_in_a_blocked_context_is_removed_with_its_tails_kept(self):
        # Tests [@ANCHOR: zero_sudo:svg_in_blocked_context]
        doc = lxml.html.fromstring("<div><p>a<svg></svg>b</p></div>")
        paragraph = doc.find("p")
        self.assertFalse(svg._in_blocked_context(paragraph))
        stash = svg._detach_svgs(doc, "n0nce", ())
        paragraph.tag = "textarea"
        self.assertTrue(svg._in_blocked_context(paragraph))
        svg._reattach_svgs(doc, stash, "n0nce")
        self.assertEqual(list(doc.iter("svg")), [])
        self.assertEqual(paragraph.text, "ab")

    def test_17_remove_keeping_tails_and_store_text_merge_tail_text(self):
        # Tests [@ANCHOR: zero_sudo:svg_remove_keeping_tails]
        # Tests [@ANCHOR: zero_sudo:svg_store_text]
        parent = etree.Element("div")
        parent.text = "a"
        first = etree.Comment("m")
        first.tail = "b"
        parent.append(first)
        keeper = etree.SubElement(parent, "e")
        keeper.tail = "c"
        second = etree.Comment("m")
        second.tail = "d"
        parent.append(second)
        svg._remove_keeping_tails(parent, {first, second})
        self.assertEqual((parent.text, keeper.tail, len(parent)), ("ab", "cd", 1))
        svg._store_text(parent, None, ["x", "y"])
        self.assertEqual(parent.text, "xy")
        svg._store_text(parent, keeper, [""])
        self.assertIsNone(keeper.tail)

    def test_18_looks_hostile_tells_attacks_from_editor_leftovers(self):
        # Tests [@ANCHOR: zero_sudo:svg_looks_hostile]
        self.assertTrue(svg._looks_hostile(self._svg("<svg><script/></svg>")))
        self.assertTrue(svg._looks_hostile(self._svg('<svg><rect onclick="x"/></svg>')))
        self.assertTrue(svg._looks_hostile(self._svg('<svg><a href="java script:x"/></svg>')))
        self.assertFalse(svg._looks_hostile(self._svg('<svg><rect aria-label="Data: voltage"/></svg>')))
        self.assertFalse(svg._looks_hostile(self._svg("<svg><metadata/></svg>")))

    def test_19_drop_keeping_tail_hands_the_tail_to_the_neighbour(self):
        # Tests [@ANCHOR: zero_sudo:svg_drop_keeping_tail]
        parent = etree.fromstring("<div>a<b>x</b>t<i/></div>")
        svg._drop_keeping_tail(parent.find("b"))
        self.assertEqual((parent.text, len(parent)), ("at", 1))
        parent = etree.fromstring("<div><i/><b/>t</div>")
        svg._drop_keeping_tail(parent.find("b"))
        self.assertEqual(parent.find("i").tail, "t")
        svg._drop_keeping_tail(parent)  # a root has no parent: nothing happens

    def test_20_sanitize_xml_svgs_rebuilds_blocks_and_reports_hostility(self):
        # Tests [@ANCHOR: zero_sudo:svg_sanitize_xml_svgs]
        root = etree.fromstring(
            '<div><svg xmlns="http://www.w3.org/2000/svg"><rect width="5"/><script/></svg>tail</div>'
        )
        self.assertTrue(svg.sanitize_xml_svgs(root))
        text = etree.tostring(root).decode()
        self.assertIn("rect", text)
        self.assertNotIn("script", text)
        self.assertTrue(text.endswith("tail</div>"))
        harmless = etree.fromstring('<div><svg xmlns="http://www.w3.org/2000/svg"><metadata/></svg></div>')
        self.assertFalse(svg.sanitize_xml_svgs(harmless))
        foreign = etree.fromstring('<div><x:svg xmlns:x="urn:other"><rect/></x:svg>t</div>')
        self.assertTrue(svg.sanitize_xml_svgs(foreign))
        self.assertEqual(len(foreign), 0)
        self.assertEqual(foreign.text, "t")

    def test_21_install_is_idempotent(self):
        # Tests [@ANCHOR: zero_sudo:svg_install]
        call = mail._Cleaner.__call__
        svg.install()
        self.assertIs(mail._Cleaner.__call__, call)
        self.assertTrue(call._hams_svg_allowlist)
