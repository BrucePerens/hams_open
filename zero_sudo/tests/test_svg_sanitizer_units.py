# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Database-free boundary tests of `zero_sudo.svg_sanitizer`, added to kill the mutation survivors of the second mutation
pass (2026-11-13): every limit at its exact edge, every validator's accept/reject set, the budgets, the text rules, the
placeholder swap around Odoo's cleaner and the XML-tree entry point."""
import logging
import secrets
from types import SimpleNamespace

import lxml.html
from lxml import etree

from odoo.tests.common import BaseCase, tagged
from odoo.tools import mail

from odoo.addons.zero_sudo import svg_sanitizer as svg
from odoo.addons.zero_sudo.tests.common import SafePatchMixin

NS = svg.SVG_NS
XML_SPACE = "{%s}space" % svg.XML_NS


def _el(xml):
    return etree.fromstring(xml)


def _clean(xml, **kwargs):
    return svg.sanitize_svg_element(_el(xml), **kwargs)


def _dump(element):
    return etree.tostring(element, encoding="unicode")


@tagged("post_install", "-at_install")
class TestScalarValidators(BaseCase):

    def test_enum_and_regex_limits(self):
        check = svg._check_enum("fill-rule")
        self.assertEqual((check("evenodd"), check("nonzero"), check("other"), check(" evenodd")), ("evenodd", "nonzero", None, None))
        regex = svg._check_regex(svg._RE_NUMBER, 5)
        self.assertEqual((regex(" 12345 "), regex("123456"), regex("x")), ("12345", None, None))
        self.assertEqual(svg._check_regex(svg._RE_NUMBER)(" 7 "), "7")
        self.assertEqual(svg._check_regex(svg._RE_NUMBER)("1" * svg.SCALAR_LIMIT), "1" * svg.SCALAR_LIMIT)
        self.assertIsNone(svg._check_regex(svg._RE_NUMBER)("1" * (svg.SCALAR_LIMIT + 1)))

    def test_paint(self):
        self.assertEqual(svg._paint(" red "), "red")
        self.assertEqual(svg._paint("#fff"), "#fff")
        self.assertEqual(svg._paint("url(#a)"), "url(#a)")
        self.assertEqual(svg._paint("url( #a )  red"), "url(#a) red")
        self.assertIsNone(svg._paint("url(http://x)"))
        self.assertIsNone(svg._paint("url(#a) url(#b)"))
        pad = " " * (2 * svg.SCALAR_LIMIT - len("url(#a)") - len("red"))
        self.assertEqual(svg._paint("url(#a)" + pad + "red"), "url(#a) red")  # 240 characters
        self.assertIsNone(svg._paint("url(#a)" + pad + " red"))  # 241

    def test_url_ref(self):
        self.assertEqual(svg._url_ref("none"), "none")
        self.assertEqual(svg._url_ref(" none "), "none")
        self.assertEqual(svg._url_ref("url(#a)"), "url(#a)")
        self.assertEqual(svg._url_ref(" url( #a ) "), "url(#a)")
        self.assertIsNone(svg._url_ref("url(a)"))
        self.assertIsNone(svg._url_ref("red"))
        self.assertEqual(svg._url_ref("url(#a)" + " " * (2 * svg.SCALAR_LIMIT - 7)), "url(#a)")
        self.assertIsNone(svg._url_ref("url(#a)" + " " * (2 * svg.SCALAR_LIMIT - 6)))

    def test_viewbox(self):
        self.assertEqual(svg._viewbox("0 0 10 20"), "0 0 10 20")
        self.assertEqual(svg._viewbox(" 0,0,10.5,-2e3 "), "0 0 10.5 -2e3")
        for bad in ("0 0 10", "0 0 10 20 30", "a b c d", "0 0 10 x", ""):
            self.assertIsNone(svg._viewbox(bad), bad)
        self.assertEqual(svg._viewbox("0 0 1 1" + " " * (2 * svg.SCALAR_LIMIT - 7)), "0 0 1 1")
        self.assertIsNone(svg._viewbox("0 0 1 1" + " " * (2 * svg.SCALAR_LIMIT - 6)))

    def test_safe_id(self):
        self.assertEqual(svg._safe_id(" a1 "), "a1")
        self.assertEqual(svg._safe_id("a__b"), "a__b")
        self.assertEqual(svg._safe_id("a" * svg.SCALAR_LIMIT), "a" * svg.SCALAR_LIMIT)
        self.assertIsNone(svg._safe_id("a" * (svg.SCALAR_LIMIT + 1)))
        for bad in ("__proto", "_a", "1a", "a b", "", "Cookie", "LOCATION", "name", "style", "constructor", "getElementById"):
            self.assertIsNone(svg._safe_id(bad), bad)
        self.assertEqual(svg._safe_id("cookies"), "cookies")

    def test_aria(self):
        self.assertEqual(svg._aria("A chart"), "A chart")
        self.assertEqual(svg._aria(""), "")
        for bad in ("a<b", 'a"b', "a'b", "a&b", "a\\b", "a`b", "a\x01b", "x" * 201):
            self.assertIsNone(svg._aria(bad), repr(bad))
        self.assertEqual(svg._aria("x" * 200), "x" * 200)

    def test_small_margin(self):
        for good in ("0", "5px", " 99 ", "99px", "9em", "9rem", "9.00em", "0.5em"):
            self.assertEqual(svg._small_margin(good), good.strip(), good)
        for bad in ("99.5", "99.99px", "9.5em", "10em", "10rem", "50em", "100", "-1px", "5%", "5pt", "abc", "", "1.234"):
            self.assertIsNone(svg._small_margin(bad), bad)

    def test_svg_size(self):
        for good in ("auto", " auto ", "0", "4000", "4000px", "100%", "250em", "250rem", "1.5em", "12345"[:4], "99.999%"[:2] + "%"):
            self.assertEqual(svg._svg_size(good), good.strip(), good)
        for bad in ("4001", "4000.5", "4001px", "100.5%", "101%", "251em", "250.5rem", "251rem", "-1", "1e3", "auto1", "", "5pt", "12345.6789"):
            self.assertIsNone(svg._svg_size(bad), bad)


@tagged("post_install", "-at_install")
class TestStyleAndAttributes(BaseCase):

    def test_clean_style(self):
        self.assertEqual(svg._clean_style("fill:red;stroke:blue"), "fill:red;stroke:blue")
        self.assertEqual(svg._clean_style(" FILL : red ; bogus:1 ; nocolon ; stroke-width:2;"), "fill:red;stroke-width:2")
        self.assertEqual(svg._clean_style("fill:javascript;stroke:red"), "stroke:red")
        self.assertEqual(svg._clean_style("fill:url(#a);margin-top:5px;background-color:#fff"), "fill:url(#a);margin-top:5px;background-color:#fff")
        self.assertIsNone(svg._clean_style("margin-top:500px"))
        self.assertIsNone(svg._clean_style("bogus:1"))
        self.assertIsNone(svg._clean_style(""))
        self.assertIsNone(svg._clean_style("fill:"))
        for bad in ("fill:red/*x*/", "fill:red;/*", "fill:red;stroke:@x", "fill:red\\", "a@b:fill"):
            self.assertIsNone(svg._clean_style(bad), bad)
        edge = "fill:red;" + " " * (2000 - len("fill:red;"))
        self.assertEqual(svg._clean_style(edge), "fill:red")
        self.assertIsNone(svg._clean_style(edge + " "))

    def test_clean_attribute_shapes(self):
        for name, value in ((5, "x"), ("fill", 5), (None, "red"), ("fill", None), (b"fill", "red")):
            self.assertIsNone(svg._clean_attribute(name, value))
        self.assertEqual(svg._clean_attribute("FILL", "red"), ("fill", "red"))
        self.assertEqual(svg._clean_attribute("viewbox", "0 0 1 1"), ("viewBox", "0 0 1 1"))
        self.assertEqual(svg._clean_attribute("ARIA-LABEL", "A chart"), ("aria-label", "A chart"))
        self.assertEqual(svg._clean_attribute("aria-hidden", "true"), ("aria-hidden", "true"))
        self.assertEqual(svg._clean_attribute("aria-roledescription", "x"), ("aria-roledescription", "x"))
        self.assertIsNone(svg._clean_attribute("aria-labelledby", "x"))
        self.assertIsNone(svg._clean_attribute("aria-label", "a<b"))
        self.assertEqual(svg._clean_attribute("style", "fill:red"), ("style", "fill:red"))
        self.assertIsNone(svg._clean_attribute("style", "bogus:1"))
        self.assertIsNone(svg._clean_attribute("onclick", "x()"))
        self.assertIsNone(svg._clean_attribute("href", "x"))
        self.assertIsNone(svg._clean_attribute("fill", "javascript"))
        self.assertIsNone(svg._clean_attribute("fill", "nope(1)"))

    def test_attribute_value_length_limit(self):
        edge = "0" * svg.MAX_ATTR_VALUE
        self.assertEqual(svg._clean_attribute("d", edge), ("d", edge))
        self.assertIsNone(svg._clean_attribute("d", edge + "0"))

    def test_local_names(self):
        for tag, expected in (("rect", "rect"), ("RECT", "rect"), ("{%s}Rect" % NS, "rect"), ("{http://other}rect", None), ("svg:rect", None),
                              ("{}rect", None), (5, None), (None, None), ("{%s" % NS, "")):
            self.assertEqual(svg.svg_local_name(tag), expected, tag)
        self.assertIsNone(svg.svg_local_name(etree.Comment("x").tag))


@tagged("post_install", "-at_install")
class TestBudgets(BaseCase):

    def test_element_count(self):
        budget = svg._Budget()
        for _ in range(svg.MAX_SVG_ELEMENTS):
            budget.element()
        with self.assertRaises(svg._Rejected) as caught:
            budget.element()
        self.assertEqual(str(caught.exception), "too many elements")

    def test_size_and_per_call_totals(self):
        budget = svg._Budget()
        budget.spend(svg.MAX_SVG_BYTES)
        with self.assertRaises(svg._Rejected) as caught:
            budget.spend(1)
        self.assertEqual(str(caught.exception), "block too large")
        totals = svg._CallTotals()
        self.assertEqual((totals.blocks, totals.size), (0, 0))
        one, two = svg._Budget(totals), svg._Budget(totals)
        one.spend(svg.MAX_SVG_BYTES)
        for _ in range(3):
            two.spend(svg.MAX_SVG_BYTES)
            two.size = 0
        self.assertEqual(totals.size, 4 * svg.MAX_SVG_BYTES)
        self.assertEqual(svg.MAX_TOTAL_SVG_BYTES, 4 * svg.MAX_SVG_BYTES)
        with self.assertRaises(svg._Rejected) as caught:
            two.spend(1)
        self.assertEqual(str(caught.exception), "too much SVG in one value")
        self.assertEqual((svg._Budget().totals, svg._Budget(totals).totals is totals), (None, True))


@tagged("post_install", "-at_install")
class TestRebuild(SafePatchMixin, BaseCase):

    def test_a_small_drawing_is_rebuilt_attribute_by_attribute(self):
        out = _clean('<svg xmlns="%s" width="100%%" height="auto" viewBox="0 0 10 20" transform="rotate(1)" onload="x()" class="c"><rect x="1" y="2" '
                     'fill="RED" style="fill:blue;bogus:1" aria-label="r" data-x="1"/></svg>' % NS)
        self.assertEqual((out.tag, dict(out.attrib)), ("svg", {"width": "100%", "height": "auto", "viewBox": "0 0 10 20", "xmlns": NS}))
        rect = out[0]
        self.assertEqual((rect.tag, dict(rect.attrib)), ("rect", {"x": "1", "y": "2", "fill": "RED", "style": "fill:blue", "aria-label": "r"}))

    def test_svg_width_and_height_are_bounded_and_transform_is_dropped_only_on_the_svg(self):
        out = _clean('<svg xmlns="%s" width="4001" height="4000px"><g transform="translate(1 2)" width="5000"/></svg>' % NS)
        self.assertEqual(dict(out.attrib), {"height": "4000px", "xmlns": NS})
        self.assertEqual(dict(out[0].attrib), {"transform": "translate(1 2)", "width": "5000"})
        self.assertEqual(dict(_clean('<svg xmlns="%s" width="12" height="13"/>' % NS).attrib), {"width": "12", "height": "13", "xmlns": NS})

    def test_xml_space_is_kept_in_its_namespace_and_checked(self):
        out = _clean('<svg xmlns="%s" xmlns:xml="http://www.w3.org/XML/1998/namespace"><text xml:space="preserve">a</text><text xml:space="bad">b</text></svg>' % NS)
        self.assertEqual(out[0].get(XML_SPACE), "preserve")
        self.assertEqual(dict(out[1].attrib), {})
        self.assertEqual(out[0].attrib.keys(), [XML_SPACE])

    def test_attributes_a_field_refuses_are_dropped(self):
        out = _clean('<svg xmlns="%s"><rect fill="red" style="fill:blue" stroke="red"/></svg>' % NS, dropped_attributes=("style", "fill"))
        self.assertEqual(dict(out[0].attrib), {"stroke": "red"})

    def test_only_allowlisted_svg_elements_survive_with_their_canonical_names(self):
        out = _clean('<svg xmlns="%s"><lineargradient id="g"><stop offset="0"/></lineargradient><script>x</script><foreignObject/><x:y xmlns:x="http://o"/>'
                     '<RECT/></svg>' % NS)
        self.assertEqual([c.tag for c in out], ["linearGradient", "rect"])
        self.assertIsNone(svg.sanitize_svg_element(_el("<rect/>")))
        self.assertIsNone(svg.sanitize_svg_element(_el('<svg xmlns="http://other"/>')))
        self.assertEqual(out.get("xmlns"), NS)

    def test_the_element_count_edge(self):
        body = "<rect/>" * (svg.MAX_SVG_ELEMENTS - 1)
        self.assertEqual(len(_clean('<svg xmlns="%s">%s</svg>' % (NS, body))), svg.MAX_SVG_ELEMENTS - 1)
        with self.assertLogs(svg._logger, level="WARNING") as logs:
            self.assertIsNone(_clean('<svg xmlns="%s">%s</svg>' % (NS, body + "<rect/>")))
        self.assertEqual(logs.output, ["WARNING:%s:SVG block dropped: too many elements" % svg._logger.name])

    def test_the_depth_edge(self):
        def nested(levels):
            return '<svg xmlns="%s">%s%s</svg>' % (NS, "<g>" * levels, "</g>" * levels)
        self.assertIsNotNone(_clean(nested(svg.MAX_SVG_DEPTH - 1)))  # the svg itself is depth 1
        with self.assertLogs(svg._logger, level="WARNING") as logs:
            self.assertIsNone(_clean(nested(svg.MAX_SVG_DEPTH)))
        self.assertEqual(logs.output, ["WARNING:%s:SVG block dropped: nested too deep" % svg._logger.name])

    def test_the_block_size_edge_counts_names_text_and_attributes(self):
        room = svg.MAX_SVG_BYTES - len("svg") - len("title")
        self.assertIsNotNone(_clean('<svg xmlns="%s"><title>%s</title></svg>' % (NS, "a" * room)))
        with self.assertLogs(svg._logger, level="WARNING") as logs:
            self.assertIsNone(_clean('<svg xmlns="%s"><title>%s</title></svg>' % (NS, "a" * (room + 1))))
        self.assertEqual(logs.output, ["WARNING:%s:SVG block dropped: block too large" % svg._logger.name])
        def paths(n):
            return "".join('<path d="%s"/>' % ("0" * svg.MAX_ATTR_VALUE) for _ in range(n))
        self.assertIsNotNone(_clean('<svg xmlns="%s">%s</svg>' % (NS, paths(13))))
        with self.assertLogs(svg._logger, level="WARNING"):
            self.assertIsNone(_clean('<svg xmlns="%s">%s</svg>' % (NS, paths(14))))

    def test_text_is_kept_only_in_text_elements_and_control_characters_go(self):
        out = _clean('<svg xmlns="%s">stray<g>word <text>ab<tspan>c</tspan>d</text>  <title></title>\n</g>   </svg>' % NS)
        self.assertIsNone(out.text)
        g = out[0]
        self.assertEqual((g.text, g[0].text, g[0][0].text, g[0][0].tail, g[0].tail, g[1].text, g[1].tail), (None, "ab", "c", "d", "\n", None, "\n"))
        self.assertEqual(g.tail, "\n")

    def test_the_tails_of_dropped_elements_are_merged(self):
        out = _clean('<svg xmlns="%s"><g/>  <script/> <g/></svg>' % NS)
        self.assertEqual([c.tag for c in out], ["g", "g"])
        self.assertEqual(out[0].tail, "\n")
        out = _clean('<svg xmlns="%s"><text>a<script/>b</text><text><tspan>x</tspan>1<script/>2</text><text><script/>z</text></svg>' % NS)
        self.assertEqual(out[0].text, "ab")
        self.assertEqual((out[1][0].tail, out[2].text), ("12", "z"))
        out = _clean('<svg xmlns="%s"><g><script/>  </g></svg>' % NS)
        self.assertEqual(out[0].text, "\n")
        out = _clean('<svg xmlns="%s"><g>q<script/>  </g></svg>' % NS)
        self.assertEqual(out[0].text, "\n")

    def test_join_and_clean_text(self):
        self.assertEqual(svg._join_text(None, "x", True), "x")
        self.assertEqual(svg._join_text("a", "b", False), "ab")
        self.assertEqual(svg._join_text("", "  ", False), "\n")
        self.assertEqual(svg._join_text(None, "  ", True), "  ")
        self.assertEqual(svg._join_text("a", " ", False), "a ")
        self.assertEqual(svg._join_text(" ", "", False), "\n")
        budget = svg._Budget()
        self.assertIsNone(svg._clean_text(None, True, budget))
        self.assertIsNone(svg._clean_text("", True, budget))
        self.assertEqual(svg._clean_text("ab\x00c\x1f", True, budget), "abc")
        self.assertEqual(budget.size, 3)
        self.assertIsNone(svg._clean_text("\x01\x02", True, budget))
        self.assertEqual(svg._clean_text("  \n", False, budget), "\n")
        self.assertIsNone(svg._clean_text(" x ", False, budget))
        self.assertEqual(budget.size, 3)
        self.assertEqual(svg._clean_text("a￾b￿c", True, svg._Budget()), "abc")

    def test_blocks_past_the_per_call_limit_are_dropped_with_one_warning(self):
        totals = svg._CallTotals()
        source = _el('<svg xmlns="%s"/>' % NS)
        with self.assertLogs(svg._logger, level="WARNING") as logs:
            results = [svg.sanitize_svg_element(source, _totals=totals) for _ in range(svg.MAX_TOTAL_SVG_BLOCKS + 3)]
        self.assertEqual([r is not None for r in results].count(True), svg.MAX_TOTAL_SVG_BLOCKS)
        self.assertTrue(all(r is not None for r in results[:svg.MAX_TOTAL_SVG_BLOCKS]))
        self.assertEqual(totals.blocks, svg.MAX_TOTAL_SVG_BLOCKS + 3)
        self.assertEqual(logs.output, ["WARNING:%s:SVG blocks past the per-value limit dropped" % svg._logger.name])
        self.assertEqual(svg.MAX_TOTAL_SVG_BLOCKS, 1000)

    def test_the_output_does_not_share_the_source(self):
        source = _el('<svg xmlns="%s"><rect/></svg>' % NS)
        out = svg.sanitize_svg_element(source)
        self.assertIsNot(out, source)
        self.assertIsNone(source.get("xmlns"))
        self.assertEqual(out.get("xmlns"), NS)


@tagged("post_install", "-at_install")
class TestPlaceholderSwap(SafePatchMixin, BaseCase):

    def doc(self, html):
        return lxml.html.fromstring(html)

    def test_top_level_blocks_only(self):
        doc = self.doc("<div><svg><svg id='inner'/></svg><p><svg/></p></div>")
        found = svg._top_level_svgs(doc)
        self.assertEqual([e.getparent().tag for e in found], ["div", "p"])
        self.assertEqual(svg._top_level_svgs(self.doc("<p>none</p>")), [])

    def test_detach_swaps_each_block_for_a_comment_and_keeps_tails(self):
        doc = self.doc("<div>a<svg><rect/></svg>tail<p><svg/>t2</p></div>")
        stash = svg._detach_svgs(doc, "NONCE", ())
        self.assertEqual(sorted(stash), [0, 1])
        comments = [c for c in doc.iter(etree.Comment)]
        self.assertEqual([c.text for c in comments], ["hamssvgNONCEx0x", "hamssvgNONCEx1x"])
        self.assertEqual((comments[0].tail, comments[1].tail), ("tail", "t2"))
        self.assertEqual(doc.text, "a")
        self.assertIsNone(doc.find(".//svg"))

    def test_detach_when_the_document_is_the_svg(self):
        doc = self.doc('<svg id="x" class="y">text<g/>tail</svg>')
        doc.tail = "after"
        stash = svg._detach_svgs(doc, "N", ())
        self.assertEqual((doc.tag, dict(doc.attrib), doc.text, doc.tail), ("div", {}, None, None))
        self.assertEqual([c.text for c in doc], ["hamssvgNx0x"])
        self.assertEqual(len(doc), 1)
        self.assertEqual(list(stash), [0])

    def test_detached_blocks_are_validated_with_the_refused_attributes(self):
        doc = self.doc('<div><svg><rect fill="red" stroke="blue"/></svg></div>')
        stash = svg._detach_svgs(doc, "N", ("fill",))
        self.assertEqual(dict(stash[0][0].attrib), {"stroke": "blue"})

    def test_reattach_puts_blocks_back_and_removes_every_placeholder(self):
        doc = self.doc("<div>a<svg><rect/></svg>tail<p><svg/>t2</p><textarea><svg/>t3</textarea></div>")
        stash = svg._detach_svgs(doc, "N", ())
        svg._reattach_svgs(doc, stash, "N")
        self.assertEqual(len(doc.findall(".//svg")), 2)
        self.assertEqual(doc.find("svg").tail, "tail")
        self.assertEqual(doc.find("p/svg").tail, "t2")
        self.assertEqual(list(doc.iter(etree.Comment)), [])
        textarea = doc.find("textarea")
        self.assertEqual((len(textarea), textarea.text), (0, "t3"))  # a blocked context drops the block, keeps the text

    def test_a_dropped_block_leaves_its_text_and_a_stray_placeholder_is_ignored(self):
        doc = self.doc("<div>a<svg onclick='x'><g/></svg>b<p>c</p><!--other--><!--hamssvgNx99x--></div>")
        stash = svg._detach_svgs(doc, "N", ())
        stash[0] = None
        svg._reattach_svgs(doc, stash, "N")
        self.assertEqual(doc.text_content(), "abc")
        self.assertEqual([c.text for c in doc.iter(etree.Comment)], ["other"])  # the unknown index is left alone: not ours to remove... but it is a placeholder of this nonce

    def test_remove_keeping_tails(self):
        parent = etree.fromstring("<p>a<x/>1<y/>2<c1/>3<c2/>4<z/>5</p>")
        comments = {parent[2], parent[3]}
        svg._remove_keeping_tails(parent, comments)
        self.assertEqual([c.tag for c in parent], ["x", "y", "z"])
        self.assertEqual((parent.text, parent[0].tail, parent[1].tail, parent[2].tail), ("a", "1", "234", "5"))
        parent = etree.fromstring("<p>a<c1/>1<c2/>2<x/>3</p>")
        svg._remove_keeping_tails(parent, {parent[0], parent[1]})
        self.assertEqual((parent.text, parent[0].tail), ("a12", "3"))
        parent = etree.fromstring("<p><c1/>1</p>")
        svg._remove_keeping_tails(parent, {parent[0]})
        self.assertEqual((len(parent), parent.text), (0, "1"))
        parent = etree.fromstring("<p><c1/></p>")
        svg._remove_keeping_tails(parent, {parent[0]})
        self.assertIsNone(parent.text)

    def test_store_text(self):
        parent, holder = etree.fromstring("<p/>"), etree.fromstring("<h/>")
        svg._store_text(parent, None, ["a", "b"])
        self.assertEqual(parent.text, "ab")
        svg._store_text(parent, holder, ["x", "y"])
        self.assertEqual((holder.tail, parent.text), ("xy", "ab"))
        svg._store_text(parent, None, ["", ""])
        self.assertIsNone(parent.text)

    def test_blocked_contexts(self):
        doc = self.doc("<div><p><svg/></p><textarea><b id='t'/></textarea><TITLE/></div>")
        self.assertFalse(svg._in_blocked_context(doc.find("p")))
        self.assertTrue(svg._in_blocked_context(doc.find("textarea/b")))
        self.assertTrue(svg._in_blocked_context(doc.find("textarea")))
        self.assertFalse(svg._in_blocked_context(doc))
        comment = etree.Comment("c")
        doc.find("p").append(comment)
        self.assertFalse(svg._in_blocked_context(comment))

    def test_clean_document_runs_the_cleaner_around_the_swap(self):
        seen = []
        self.assertEqual(svg.clean_document(self.doc("<p>no svg</p>"), lambda d: seen.append("plain") or "R1"), "R1")
        asked = []
        real = secrets.token_hex
        self.safe_patch_object(svg.secrets, "token_hex", new=lambda n: asked.append(n) or real(n))
        doc = self.doc("<div>a<svg><rect/></svg>b</div>")

        def cleaner(tree):
            seen.append([c.text for c in tree.iter(etree.Comment)])
            return "R2"
        self.assertEqual(svg.clean_document(doc, cleaner), "R2")
        self.assertEqual(asked, [12])
        self.assertTrue(seen[1][0].startswith("hamssvg") and len(seen[1][0]) == len("hamssvg") + 24 + len("x0x"))
        self.assertEqual(len(doc.findall("svg")), 1)
        self.assertEqual(list(doc.iter(etree.Comment)), [])

    def test_install_passes_the_refused_attributes_to_the_wrapper(self):
        calls = []
        self.safe_patch_object(svg, "clean_document", new=lambda doc, run, dropped: calls.append((doc, dropped)) or "R")
        wrapper = mail._Cleaner.__call__
        for style, classes, expected in ((True, True, ("style", "class")), (True, False, ("style",)), (False, True, ("class",)), (False, False, ())):
            self.assertEqual(wrapper(SimpleNamespace(style=style, strip_classes=classes), "DOC"), "R")
            self.assertEqual(calls[-1], ("DOC", expected))


@tagged("post_install", "-at_install")
class TestXmlEntryPoint(BaseCase):

    def run_tree(self, xml):
        root = _el(xml)
        return root, svg.sanitize_xml_svgs(root)

    def test_hostile_markup_is_reported_and_removed_while_harmless_parts_are_not(self):
        for hostile in ("<script/>", "<foreignObject/>", "<use/>", "<animate/>", "<set/>", "<style/>", "<iframe/>", "<image/>",
                        '<rect onclick="x"/>', '<rect ONLOAD="x"/>', '<a href="javascript:alert(1)"/>', '<rect style="a:b" href="java\\nscript:1"/>',
                        '<rect href="VBScript:x"/>', '<rect src="data:text/html,x"/>', '<rect xlink:href="javascript:1" xmlns:xlink="http://x"/>',
                        '<x:script xmlns:x="http://x"/>', "<HTML/>", "<body/>"):
            root, found = self.run_tree('<p><svg xmlns="%s">%s</svg></p>' % (NS, hostile.replace("\\n", "\n")))
            self.assertIs(found, True, hostile)
        for harmless in ("<rect/>", '<rect fill="red" data-x="1"/>', '<rect href="https://ok"/>', "<g><title>t</title></g>"):
            root, found = self.run_tree('<p><svg xmlns="%s">%s</svg></p>' % (NS, harmless))
            self.assertIs(found, False, harmless)
        root, found = self.run_tree('<p><svg xmlns="%s"><rect/><sodipodi:namedview xmlns:sodipodi="http://x"/></svg></p>' % NS)
        self.assertIs(found, False)

    def test_the_block_is_replaced_by_its_rebuild_and_keeps_its_tail(self):
        root, found = self.run_tree('<p>a<svg xmlns="%s" onload="x"><rect/></svg>tail<b/></p>' % NS)
        self.assertIs(found, True)
        block = root.find("svg") if root.find("svg") is not None else root[0]
        self.assertEqual((block.tail, block.get("xmlns"), block.get("onload")), ("tail", NS, None))
        self.assertEqual(root.text, "a")

    def test_nested_blocks_count_once_and_foreign_namespace_blocks_are_removed(self):
        root, found = self.run_tree('<p><svg xmlns="%s"><svg><rect/></svg></svg></p>' % NS)
        self.assertIs(found, False)
        self.assertEqual(len(list(root.iter("svg"))), 2)  # one rebuild of the outer block; the inner svg is an allowed child
        root, found = self.run_tree('<p>a<svg xmlns="http://other"><g/></svg>t<b/></p>')
        self.assertIs(found, True)
        self.assertEqual((len(root), root.text), (1, "at"))

    def test_too_many_blocks_is_hostile(self):
        body = "".join('<svg xmlns="%s"/>' % NS for _ in range(svg.MAX_TOTAL_SVG_BLOCKS))
        root, found = self.run_tree("<p>%s</p>" % body)
        self.assertIs(found, False)  # exactly the limit is fine
        with self.assertLogs(svg._logger, level="WARNING"):
            root, found = self.run_tree("<p>%s<svg xmlns=\"%s\"/></p>" % (body, NS))
        self.assertIs(found, True)
        self.assertEqual(len(root), svg.MAX_TOTAL_SVG_BLOCKS)

    def test_an_oversize_block_is_removed_without_being_hostile(self):
        room = svg.MAX_SVG_BYTES
        with self.assertLogs(svg._logger, level="WARNING"):
            root, found = self.run_tree('<p>a<svg xmlns="%s"><title>%s</title></svg>b</p>' % (NS, "x" * room))
        self.assertIs(found, False)
        self.assertEqual((len(root), root.text), (0, "ab"))

    def test_drop_keeping_tail(self):
        root = etree.fromstring("<p>a<x/>1<y/>2</p>")
        svg._drop_keeping_tail(root[0])
        self.assertEqual((root.text, [c.tag for c in root], root[0].tail), ("a1", ["y"], "2"))
        root = etree.fromstring("<p><x/>1<y/>2<z/>3</p>")
        svg._drop_keeping_tail(root[1])
        self.assertEqual((root.text, root[0].tail, root[1].tail), (None, "12", "3"))
        root = etree.fromstring("<p><x/><y/></p>")
        svg._drop_keeping_tail(root[1])
        self.assertEqual(len(root), 1)
        lonely = etree.fromstring("<p/>")
        lonely.tail = "t"
        self.assertIsNone(svg._drop_keeping_tail(lonely))
        self.assertEqual(lonely.tail, "t")

    def test_looks_hostile_ignores_comments_and_reads_local_names(self):
        self.assertFalse(svg._looks_hostile(_el('<svg xmlns="%s"><!--script--><rect/></svg>' % NS)))
        self.assertTrue(svg._looks_hostile(_el('<svg xmlns="%s" xmlns:s="http://x"><s:Script/></svg>' % NS)))
        self.assertTrue(svg._looks_hostile(_el('<svg xmlns="%s" xmlns:x="http://x" x:onclick="1"/>' % NS)))
        self.assertTrue(svg._looks_hostile(_el('<svg xmlns="%s" href=" j a v a s c r i p t : x"/>' % NS)))
        self.assertFalse(svg._looks_hostile(_el('<svg xmlns="%s" title="javascript:x" fill="data:x"/>' % NS)))
        self.assertFalse(svg._looks_hostile(_el('<svg xmlns="%s" href="/javascript/x"/>' % NS)))
        self.assertTrue(svg._looks_hostile(_el('<svg xmlns="%s" opacity="1" ontology="no"/>' % NS)))  # any attribute starting with "on"
