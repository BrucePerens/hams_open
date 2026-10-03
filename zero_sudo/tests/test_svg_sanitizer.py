# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for zero_sudo's SVG allowlist sanitizer.

Tests [@ANCHOR: zero_sudo:svg_allowlist_sanitizer]
[@ANCHOR: test_svg_allowlist_sanitizer_unit] and
[@ANCHOR: test_svg_allowlist_sanitizer_fields] live here. The real-browser half
(do the shapes render, does anything run) is in hams_com's theme_hams tests,
which need the site's own theme and CSP.
"""
import importlib
import re
import io
import time
from unittest import mock

import lxml.etree as etree
import lxml.html

from odoo.tests.common import BaseCase, TransactionCase, tagged
from odoo.tools import convert, html_sanitize, mail

from odoo.addons.zero_sudo import svg_sanitizer

from .svg_corpus import FORGED_TOKEN, SAFE_ELEMENTS, SCHEMATIC_SVG, XSS_CORPUS

SVG_NS = "http://www.w3.org/2000/svg"
_SAFE_LOWER = {name.lower() for name in SAFE_ELEMENTS}
# Elements that must never appear anywhere in cleaned output (Odoo's own sanitizer
# kills these outside SVG too). Comments are inert text and are skipped.
_NEVER_ELEMENTS = {
    "script", "iframe", "style", "foreignobject", "object", "embed", "link",
    "meta", "base",
}
_FORBIDDEN_ATTRS = {
    "href", "xlink:href", "src", "srcdoc", "action", "formaction",
    "data", "filter", "mask",
}
_DANGEROUS_VALUE = re.compile(
    r"javascript:|vbscript:|data:|url\((?!#)|@import|expression", re.IGNORECASE
)


def _outside_svg_allowed(element):
    """Elements Odoo's own sanitizer is responsible for (not this module)."""
    return not any(a.tag == "svg" for a in element.iterancestors())


def assert_output_is_safe(test, name, output):
    """The invariants every cleaned output must satisfy, whatever went in."""
    text = str(output)
    # The corpus's own forged look-alike token is legitimate text; a real leaked token
    # carries a random nonce and so never equals it.
    unforged = text.replace(FORGED_TOKEN, "")
    test.assertNotIn("hamssvg", unforged, f"{name}: placeholder token leaked")
    test.assertNotIn("evil.example", text, f"{name}: injected URL kept")
    test.assertNotIn("/etc/passwd", text, f"{name}: file entity kept")
    if not text.strip():
        return
    tree = lxml.html.fromstring(text)
    for element in tree.iter():
        if not isinstance(element.tag, str):
            continue
        tag = element.tag.lower()
        test.assertNotIn(tag, _NEVER_ELEMENTS, f"{name}: <{tag}> in output: {text[:300]}")
        inside = tag == "svg" or not _outside_svg_allowed(element)
        for attr, value in element.attrib.items():
            lowered = attr.lower()
            test.assertFalse(
                lowered.startswith("on"), f"{name}: handler {attr} kept"
            )
            if inside:
                test.assertNotIn(lowered, _FORBIDDEN_ATTRS, name)
                test.assertIsNone(
                    _DANGEROUS_VALUE.search(value),
                    f"{name}: dangerous value in {attr}={value!r}",
                )
        if inside:
            test.assertIn(tag, _SAFE_LOWER, f"{name}: <{tag}> inside svg")



class _DataFile(io.BytesIO):
    """convert_xml_import() reports errors against `xmlfile.name`."""

    name = "svg_allowlist_probe.xml"


@tagged("post_install", "-at_install")
class TestSvgAllowlistSanitizerUnit(BaseCase):
    # [@ANCHOR: test_svg_allowlist_sanitizer_unit]

    def _svg_elements(self, output):
        tree = lxml.html.fromstring(str(output))
        return [e for e in tree.iter() if isinstance(e.tag, str)]

    def test_01_patch_is_installed_exactly_once(self):
        # Tests [@ANCHOR: zero_sudo:svg_allowlist_install]
        call = mail._Cleaner.__call__
        self.assertTrue(getattr(call, "_hams_svg_allowlist", False))
        svg_sanitizer.install()
        self.assertIs(mail._Cleaner.__call__, call)
        self.assertFalse(
            getattr(call.__wrapped__, "_hams_svg_allowlist", False),
            "the patch wrapped itself",
        )

    def test_02_representative_schematic_survives_intact(self):
        output = html_sanitize(SCHEMATIC_SVG)
        text = str(output)
        names = [e.tag for e in self._svg_elements(output)]
        for shape in ("rect", "line", "circle", "polygon", "polyline",
                      "ellipse", "path", "text", "tspan", "marker"):
            self.assertIn(shape, names, f"{shape} was stripped")
        # Camel-case SVG names and attributes are restored for the browser.
        for camel in ("viewBox=", "<linearGradient", "<clipPath", "markerWidth=",
                      "refX=", "</linearGradient>"):
            self.assertIn(camel, text)
        self.assertNotIn("viewbox=", text)
        self.assertIn('viewBox="0 0 400 220"', text)
        self.assertIn('stroke="currentColor"', text)
        self.assertIn('fill="currentColor"', text)
        self.assertIn("1kΩ", text)
        self.assertIn("K1 ", text)
        self.assertIn('marker-end="url(#dot)"', text)
        self.assertIn('clip-path="url(#clipbox)"', text)
        self.assertIn('aria-label="Relay driven by an opto-isolator"', text)
        self.assertIn(f'xmlns="{SVG_NS}"', text)
        self.assertIn("background-color:#ffffff", text)
        self.assertIn("<title>Relay driven by an opto-isolator</title>", text)
        assert_output_is_safe(self, "schematic", output)

    def test_03_sanitizing_is_idempotent(self):
        once = html_sanitize(SCHEMATIC_SVG)
        self.assertEqual(html_sanitize(once), once)
        for name, payload in XSS_CORPUS:
            with self.subTest(vector=name):
                # A bare <svg> that Odoo's own html/body stripping leaves at the top
                # level is wrapped in a <div> by the next pass (the cleaner cannot
                # drop a root element); from there the value no longer changes.
                second = html_sanitize(html_sanitize(payload))
                self.assertEqual(html_sanitize(second), second)

    def test_04_lowercased_svg_from_an_older_pass_is_canonicalised(self):
        mangled = (
            '<svg viewbox="0 0 10 10"><lineargradient id="g" gradientunits="userSpaceOnUse">'
            '<stop offset="0"/></lineargradient><clippath id="c"><rect/></clippath></svg>'
        )
        text = str(html_sanitize(mangled))
        self.assertIn('viewBox="0 0 10 10"', text)
        self.assertIn("<linearGradient", text)
        self.assertIn('gradientUnits="userSpaceOnUse"', text)
        self.assertIn("<clipPath", text)

    def test_05_content_outside_svg_is_untouched(self):
        original_call = mail._Cleaner.__call__.__wrapped__
        samples = [
            "<p>plain <b>bold</b> text</p>",
            "<div><script>alert(1)</script><p onclick='x'>a</p></div>",
            "<table><tr><td><img src='a.png' onerror='x'></td></tr></table>",
            "<p>no diagram</p><a href='javascript:alert(1)'>x</a>",
        ]
        for sample in samples:
            with self.subTest(sample=sample):
                patched = html_sanitize(sample)
                with mock.patch.object(mail._Cleaner, "__call__", original_call):
                    stock = html_sanitize(sample)
                self.assertEqual(patched, stock)

    def test_06_around_svg_is_still_sanitised_by_odoo(self):
        output = html_sanitize(
            "<p onclick='x'>a</p><svg><rect/></svg><script>alert(1)</script><p>b</p>"
        )
        text = str(output)
        self.assertNotIn("onclick", text)
        self.assertNotIn("script", text)
        self.assertIn("<rect>", text)
        self.assertIn("<p>b</p>", text)

    def test_07_xss_corpus_is_neutralised(self):
        self.assertGreaterEqual(len(XSS_CORPUS), 100)
        for name, payload in XSS_CORPUS:
            with self.subTest(vector=name):
                assert_output_is_safe(self, name, html_sanitize(payload))

    def test_08_corpus_vectors_mixed_with_a_good_schematic(self):
        """A malicious block must not take a neighbouring good one down or leak into it."""
        for name, payload in XSS_CORPUS:
            with self.subTest(vector=name):
                output = html_sanitize(f"{SCHEMATIC_SVG}<hr>{payload}<hr>{SCHEMATIC_SVG}")
                assert_output_is_safe(self, name, output)
                kept = str(output).count('id="r1"')
                # libxml2 drops everything after a stray </html> or a NUL byte for every
                # input, SVG or not, and a <math> context is deliberately never expanded
                # into; only those vectors may lose the trailing copy.
                lossy = "</html>" in payload or "\x00" in payload or "<math>" in payload
                self.assertGreaterEqual(kept, 1 if lossy else 2, name)

    def test_09_specific_attack_surfaces_removed(self):
        text = str(html_sanitize(
            "<svg onload='x()' xmlns:xlink='http://www.w3.org/1999/xlink'>"
            "<a xlink:href='javascript:x()'><text>a</text></a>"
            "<use href='#z'/><image href='x.png'/><foreignObject/>"
            "<style>*{}</style><rect style='fill:red;width:expression(1)' id='k'/></svg>"
        ))
        for gone in ("onload", "xlink", "<a", "<use", "<image", "foreignobject",
                     "<style", "expression"):
            self.assertNotIn(gone, text.lower())
        self.assertIn('style="fill:red"', text)
        self.assertIn('id="k"', text)

    def test_10_only_same_document_url_fragments_survive(self):
        text = str(html_sanitize(
            "<svg><rect fill='url( #a )' stroke='url(#b) red' clip-path='url(#c)'/>"
            "<rect fill='url(https://evil.example/x#a)'/>"
            "<rect fill='url(\"#q\")'/><rect marker-end='url(x.svg#m)'/></svg>"
        ))
        self.assertIn('fill="url(#a)"', text)
        self.assertIn('stroke="url(#b) red"', text)
        self.assertIn('clip-path="url(#c)"', text)
        self.assertEqual(text.count("url("), 3)

    def test_11_namespace_is_forced_to_svg(self):
        for given in ("http://www.w3.org/1999/xhtml", "javascript:alert(1)", ""):
            text = str(html_sanitize(f"<svg xmlns='{given}'><rect/></svg>"))
            self.assertIn(f'xmlns="{SVG_NS}"', text)
            self.assertNotIn("xhtml", text)
            self.assertNotIn("javascript", text)

    def test_12_size_element_and_depth_caps_drop_the_block(self):
        many = "<svg>" + "<rect/>" * (svg_sanitizer.MAX_SVG_ELEMENTS + 1) + "</svg><p>kept</p>"
        text = str(html_sanitize(many))
        self.assertNotIn("<svg", text)
        self.assertIn("<p>kept</p>", text)
        allowed = "<svg>" + "<rect/>" * (svg_sanitizer.MAX_SVG_ELEMENTS - 1) + "</svg>"
        self.assertEqual(str(html_sanitize(allowed)).count("<rect>"), 4999)
        deep_levels = svg_sanitizer.MAX_SVG_DEPTH + 2
        deep = "<svg>" + "<g>" * deep_levels + "<rect/>" + "</g>" * deep_levels + "</svg>"
        self.assertNotIn("<svg", str(html_sanitize(deep)))
        shallow = "<svg>" + "<g>" * 10 + "<rect/>" + "</g>" * 10 + "</svg>"
        self.assertIn("<rect>", str(html_sanitize(shallow)))
        big = "<svg><text>" + "a" * (svg_sanitizer.MAX_SVG_BYTES + 10) + "</text></svg>"
        self.assertNotIn("<svg", str(html_sanitize(big)))

    def test_12b_pathological_values_are_rejected_in_linear_time(self):
        """Patterns like `\\d+\\.?\\d*` backtrack quadratically on a long digit run with a
        bad last character; a long run of spaces between transform() calls could backtrack
        exponentially. Both must be answered at once."""
        digits = "1" * 20000 + "x"
        transform = "translate(1) " * 50 + " " * 5000 + "x"
        started = time.monotonic()
        text = str(html_sanitize(
            f"<svg width='{digits}' height='{digits}'><rect x='{digits}' transform='{transform}' "
            f"fill='{digits}' stroke-width='{digits}' style='fill:{digits}'/>"
            f"<path d='{'M0 0 ' * 3000}x'/></svg>"
        ))
        self.assertLess(time.monotonic() - started, 3.0)
        self.assertIn("<rect>", text)
        self.assertNotIn("transform", text)

    def test_13_forged_token_is_plain_text(self):
        text = str(html_sanitize(f"<p>{FORGED_TOKEN}</p><svg><circle r='1'/></svg>"))
        self.assertIn(FORGED_TOKEN, text)
        self.assertEqual(text.count("<svg"), 1)
        self.assertEqual(text.count("<circle"), 1)

    def test_14_svg_is_not_expanded_where_a_browser_reads_text(self):
        attribute = str(html_sanitize("<p title='<svg><rect/></svg>'>t</p>"))
        self.assertNotIn("<svg", attribute.replace("&lt;svg", ""))
        self.assertNotIn("hamssvg", attribute)
        area = html_sanitize("<textarea><svg><rect/></svg></textarea>", sanitize_form=False)
        for element in self._svg_elements(area):
            self.assertNotEqual(element.tag, "svg")
        self.assertNotIn("hamssvg", str(area))

    def test_15_svg_survives_in_ordinary_containers(self):
        svg = "<svg viewBox='0 0 1 1'><rect width='1' height='1'/></svg>"
        for outer in ("<td>{}</td>", "<li>{}</li>", "<p>x {} y</p>", "<figure>{}</figure>",
                      "<section><div>{}</div></section>", "<span>{}</span>"):
            with self.subTest(outer=outer):
                markup = outer.format(svg)
                if outer.startswith("<td"):
                    markup = f"<table><tr>{markup}</tr></table>"
                if outer.startswith("<li"):
                    markup = f"<ul>{markup}</ul>"
                text = str(html_sanitize(markup))
                self.assertEqual(text.count("<svg"), 1, text)
                self.assertIn("viewBox", text)

    def test_16_whole_value_that_is_one_svg_is_kept(self):
        text = str(html_sanitize("<svg viewBox='0 0 1 1'><rect/></svg>"))
        self.assertEqual(text.count("<svg"), 1)
        self.assertIn("viewBox", text)

    def test_17_field_options_strip_style_and_class_is_never_kept(self):
        source = "<svg class='k' style='fill:red' fill='blue'><rect/></svg>"
        plain = str(html_sanitize(source))
        self.assertNotIn("class=", plain)
        self.assertIn('style="fill:red"', plain)
        stripped = str(html_sanitize(source, strip_style=True, strip_classes=True))
        self.assertNotIn("class=", stripped)
        self.assertNotIn("style=", stripped)
        self.assertIn('fill="blue"', stripped)

    def test_18_xml_output_method_is_well_formed_svg(self):
        output = html_sanitize(SCHEMATIC_SVG, output_method="xml")
        parser = etree.XMLParser(resolve_entities=False, no_network=True, load_dtd=False)
        root = etree.fromstring(f"<root>{output}</root>".encode(), parser)
        svg = root.find(f".//{{{SVG_NS}}}svg")
        self.assertIsNotNone(svg)
        self.assertIsNotNone(svg.find(f".//{{{SVG_NS}}}rect"))
        self.assertIsNotNone(svg.find(f".//{{{SVG_NS}}}linearGradient"))

    def test_19_two_diagrams_keep_separate_content(self):
        text = str(html_sanitize(
            "<p>a</p><svg><text>first</text></svg><p>b</p><svg><text>second</text></svg><p>c</p>"
        ))
        self.assertLess(text.index("first"), text.index("<p>b</p>"))
        self.assertLess(text.index("<p>b</p>"), text.index("second"))
        self.assertLess(text.index("second"), text.index("<p>c</p>"))

    def test_20_html_entities_in_labels_become_text(self):
        text = str(html_sanitize("<svg><text>5 &micro;F &amp; 10 &Omega; &lt;b&gt;</text></svg>"))
        self.assertIn("5 µF &amp; 10 Ω &lt;b&gt;", text)

    def test_21_per_value_totals_bound_work_and_output(self):
        value = "<svg></svg>x" * 20000
        started = time.monotonic()
        text = str(html_sanitize(value))
        self.assertLess(time.monotonic() - started, 2.0)
        self.assertEqual(text.count("<svg"), svg_sanitizer.MAX_TOTAL_SVG_BLOCKS)
        self.assertLess(len(text), 2 * len(value))
        self.assertNotIn("hamssvg", text)
        text = str(html_sanitize("<svg><text>" + "a" * 200000 + "</text></svg>") * 1)
        many = "<svg><text>" + "a" * 200000 + "</text></svg>"
        text = str(html_sanitize(many * 8))
        self.assertLessEqual(text.count("<svg"), 5)
        self.assertLess(len(text), svg_sanitizer.MAX_TOTAL_SVG_BYTES + 1000)

    def test_22_style_cannot_size_or_move_an_svg(self):
        text = str(html_sanitize(
            "<svg style='margin:-5px;margin-left:5px;margin-top:-1px;margin-bottom:12px;"
            "width:10px;height:10px;position:fixed;display:block;max-width:5px;"
            "background-color:#fff;fill:red' width='20000' height='50%' transform='translate(-9999)'>"
            "<rect/></svg>"
        ))
        self.assertIn('style="margin-bottom:12px;background-color:#fff;fill:red"', text)
        self.assertNotIn("width=", text)
        self.assertNotIn("transform", text)
        self.assertIn('height="50%"', text)
        for given in ("200%", "4001", "-5", "1e9", "5cm"):
            self.assertNotIn("width=", str(html_sanitize(f"<svg width='{given}'/>")))
        for given in ("100%", "4000", "300px", "auto"):
            self.assertIn("width=", str(html_sanitize(f"<svg width='{given}'/>")))
        self.assertNotIn("margin-bottom:100px", str(html_sanitize("<svg style='margin-bottom:100px'/>")))

    def test_23_role_aria_and_ids_cannot_reach_outside_the_diagram(self):
        text = str(html_sanitize(
            "<svg role='img' aria-label='ok' aria-hidden='false' aria-roledescription='d' "
            "aria-owns='x' aria-controls='y' aria-labelledby='z' aria-describedby='q' "
            "aria-flowto='r' aria-activedescendant='s'><rect id='ok1'/><rect id='cookie'/>"
            "<rect id='Location'/><rect id='__proto__'/><rect id='x__y'/></svg>"
        ))
        for kept in ('role="img"', 'aria-label="ok"', 'aria-hidden="false"', "aria-roledescription", 'id="ok1"', 'id="x__y"'):
            self.assertIn(kept, text)
        for gone in ("aria-owns", "aria-controls", "aria-labelledby", "aria-describedby",
                     "aria-flowto", "aria-activedescendant", 'id="cookie"', 'id="Location"', "__proto__"):
            self.assertNotIn(gone, text)
        for role in ("button", "link", "dialog", "alert"):
            self.assertNotIn("role=", str(html_sanitize(f"<svg role='{role}'/>")))
        for role in ("graphics-document", "presentation", "none"):
            self.assertIn("role=", str(html_sanitize(f"<svg role='{role}'/>")))

    def test_24_adjacent_blocks_and_surrounding_text_keep_order(self):
        text = str(html_sanitize("<p>a<svg><text>1</text></svg>b<svg><text>2</text></svg>c</p>"))
        self.assertEqual(
            re.sub(r"<[^>]+>", "|", text).replace("||", "|"), "|a|1|b|2|c|"
        )

    def test_25_block_count_boundary_is_exactly_1000(self):
        self.assertEqual(svg_sanitizer.MAX_TOTAL_SVG_BLOCKS, 1000)
        for count, expected in ((1000, 1000), (1001, 1000)):
            started = time.monotonic()
            text = str(html_sanitize("<p>" + "<svg><circle r='1'/></svg>" * count + "</p>"))
            self.assertLess(time.monotonic() - started, 2.0)
            self.assertEqual(text.count("<svg"), expected, count)
        # the 1001st (and later) blocks are the ones dropped, not an earlier one
        marked = "".join(f"<svg><text>b{i}</text></svg>" for i in range(1001))
        text = str(html_sanitize(marked))
        self.assertIn("b999<", text)
        self.assertNotIn("b1000<", text)
        self.assertNotIn("hamssvg", text)

    def test_26_far_more_than_1000_blocks_stay_fast(self):
        for value in ("<svg></svg>" * 20000, "<p>" + "<svg></svg><b>x</b>" * 20000 + "</p>"):
            started = time.monotonic()
            text = str(html_sanitize(value))
            self.assertLess(time.monotonic() - started, 2.0)
            self.assertEqual(text.count("<svg"), 1000)
            self.assertLess(len(text), 8 * len(value) + 100)


@tagged("post_install", "-at_install")
class TestSvgAllowlistHtmlFields(TransactionCase):
    # [@ANCHOR: test_svg_allowlist_sanitizer_fields]

    SAFE = (
        "<p>diagram</p><svg viewBox='0 0 40 20'>"
        "<rect id='box' x='1' y='1' width='30' height='10' stroke='currentColor' fill='none'/>"
        "<text x='5' y='8'>10k</text></svg>"
    )
    EVIL = (
        "<p>diagram</p><svg viewBox='0 0 40 20' onload='window.__xss=1'>"
        "<script>window.__xss=1</script><rect id='box' x='1' width='30' height='10'/>"
        "<a href='javascript:window.__xss=1'><text>x</text></a>"
        "<foreignObject><iframe src='https://evil.example/'></iframe></foreignObject>"
        "<use href='data:image/svg+xml,x'/></svg>"
    )

    def _assert_kept(self, value):
        text = str(value)
        self.assertIn("<svg", text)
        self.assertIn("viewBox=", text)
        self.assertIn('stroke="currentColor"', text)
        self.assertIn("<text", text)

    def _assert_neutralised(self, value):
        assert_output_is_safe(self, "field", value)
        self.assertIn('id="box"', str(value))

    def test_01_html_sanitize_bound_by_name_everywhere_is_covered(self):
        """Modules that did `from odoo.tools import html_sanitize` still see the patch,
        because the patch lives in the cleaner class they all end up calling."""
        modules = [
            "odoo.tools",
            "odoo.tools.mail",
            "odoo.orm.fields_textual",
            "odoo.addons.mail.models.mail_thread",
            "odoo.addons.zero_sudo.models.ir_module_module",
        ]
        for module_name in modules:
            with self.subTest(module=module_name):
                function = importlib.import_module(module_name).html_sanitize
                self._assert_kept(function(self.SAFE))
                self._assert_neutralised(function(self.EVIL))

    def test_02_every_html_field_on_several_models(self):
        partner_field = self.env["res.partner"]._fields["comment"]
        message_field = self.env["mail.message"]._fields["body"]
        self.assertEqual(partner_field.type, "html")
        self.assertTrue(partner_field.sanitize)
        self.assertTrue(message_field.sanitize)
        partner = self.env["res.partner"].create({"name": "SVG", "comment": self.SAFE})
        self._assert_kept(partner.comment)
        partner.write({"comment": self.EVIL})
        self._assert_neutralised(partner.comment)
        evil_partner = self.env["res.partner"].create({"name": "SVG evil", "comment": self.EVIL})
        self._assert_neutralised(evil_partner.comment)
        message = self.env["mail.message"].create({"body": self.SAFE, "message_type": "comment"})
        self._assert_kept(message.body)
        message.write({"body": self.EVIL})
        self._assert_neutralised(message.body)
        evil_message = self.env["mail.message"].create({"body": self.EVIL, "message_type": "comment"})
        self._assert_neutralised(evil_message.body)

    def test_03_data_file_load_path(self):
        """A record loaded from an XML data file goes through the same field write."""
        xml = (
            "<odoo><data>"
            '<record id="svg_allowlist_probe_safe" model="res.partner">'
            '<field name="name">SVG probe safe</field>'
            f'<field name="comment"><![CDATA[{self.SAFE}]]></field></record>'
            '<record id="svg_allowlist_probe_evil" model="res.partner">'
            '<field name="name">SVG probe evil</field>'
            f'<field name="comment"><![CDATA[{self.EVIL}]]></field></record>'
            "</data></odoo>"
        )
        convert.convert_xml_import(
            self.env, "zero_sudo", _DataFile(xml.encode()), None, "init"
        )
        safe = self.env.ref("zero_sudo.svg_allowlist_probe_safe")
        evil = self.env.ref("zero_sudo.svg_allowlist_probe_evil")
        self._assert_kept(safe.comment)
        self._assert_neutralised(evil.comment)

    def test_04_stored_value_round_trips_without_change(self):
        partner = self.env["res.partner"].create({"name": "SVG round trip", "comment": self.SAFE})
        stored = str(partner.comment)
        partner.write({"comment": stored})
        self.assertEqual(str(partner.comment), stored)
