# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
import io

from odoo.tests.common import tagged
from odoo.tools import convert

from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.addons.zero_sudo.tests.svg_corpus import SCHEMATIC_SVG, XSS_CORPUS
from odoo.addons.zero_sudo.tests.test_svg_sanitizer import (
    assert_output_is_safe,
)



class _DataFile(io.BytesIO):
    """convert_xml_import() reports errors against `xmlfile.name`."""

    name = "svg_allowlist_probe.xml"


@tagged("post_install", "-at_install")
class TestManualArticleInlineSvg(HamsHttpCase):
    # Tests [@ANCHOR: test_svg_allowlist_sanitizer_fields]
    # Tests [@ANCHOR: zero_sudo:svg_allowlist_sanitizer]
    """knowledge.article.body is a sanitized Html field; a schematic written as inline SVG
    must survive a write, an unsafe SVG must not, and the public page must serve the result."""

    def setUp(self):
        super().setUp()
        self.env["ir.config_parameter"].set_param("auth_signup.invitation_scope", "b2c")

    def _create(self, body):
        return self.env["knowledge.article"].create(
            {"name": "SVG article", "body": body, "is_published": True}
        )

    def test_01_create_and_write_keep_a_safe_diagram(self):
        article = self._create(f"<p>Intro</p>{SCHEMATIC_SVG}")
        self.assertIn('viewBox="0 0 400 220"', article.body)
        self.assertIn("<linearGradient", article.body)
        self.assertIn('stroke="currentColor"', article.body)
        article.write({"body": f"<p>Edited</p>{SCHEMATIC_SVG}"})
        self.assertIn("Edited", article.body)
        self.assertIn('id="r1"', article.body)

    def test_02_every_corpus_vector_is_neutralised_on_create_and_write(self):
        for name, payload in XSS_CORPUS:
            with self.subTest(vector=name):
                article = self._create(f"<p>x</p>{payload}")
                assert_output_is_safe(self, name, article.body or "")
                article.write({"body": f"<p>y</p>{payload}"})
                assert_output_is_safe(self, name, article.body or "")

    def test_03_data_file_load_path(self):
        xml = (
            "<odoo><data>"
            '<record id="svg_article_probe" model="knowledge.article">'
            '<field name="name">SVG data probe</field>'
            f'<field name="body"><![CDATA[<p>d</p>{SCHEMATIC_SVG}<svg onload="window.__xss=1">'
            "<script>window.__xss=1</script><rect/></svg>]]></field>"
            "</record></data></odoo>"
        )
        convert.convert_xml_import(
            self.env, "knowledge", _DataFile(xml.encode()), None, "init"
        )
        article = self.env.ref("knowledge.svg_article_probe")
        self.assertIn('id="r1"', article.body)
        assert_output_is_safe(self, "data file", article.body)

    def test_04_public_page_serves_the_diagram_untouched(self):
        """_compile_markdown must not re-parse a body whose only structure is an SVG whose
        labels look like markdown ("- 10k"), or the drawing is destroyed."""
        svg = (
            "<svg viewBox='0 0 100 20'><text x='1' y='10'>- 10k</text>"
            "<text x='1' y='19'>- 4N25</text><rect id='keep' width='5' height='5'/></svg>"
        )
        article = self._create(f"<p>**Heading** [link](https://example.com)</p>{svg}")
        self.authenticate(None, None)
        response = self.url_open(article.website_url)
        self.assertEqual(response.status_code, 200)
        self.assertIn(b'id="keep"', response.content)
        self.assertIn(b"<svg", response.content)
        self.assertIn(b"viewBox=", response.content)
