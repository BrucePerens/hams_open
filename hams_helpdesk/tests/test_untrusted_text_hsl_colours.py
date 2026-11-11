# Copyright © Bruce Perens K6BP. AGPL-3.0.
"""hsl() colours in the hidden-text check of models/untrusted_text.py."""
from odoo.tests.common import BaseCase, tagged

from odoo.addons.hams_helpdesk.models import untrusted_text as ut

C = "CANARY7731"


@tagged("post_install", "-at_install")
class TestUntrustedTextHslColours(BaseCase):
    def test_pale_hsl_text_on_a_white_page_is_hidden_text(self):
        """Only lightness 97 and over used to be read from hsl(); `color:hsl(0,0%,93%)` (contrast 1.2 on white, the same as
        #ededed, which was caught) passed with no finding."""
        self.assertEqual(ut._parse_color("hsl(0 0% 93%)"), (237, 237, 237))
        self.assertEqual(ut._parse_color("hsl(200, 10%, 90%)"), (227, 230, 232))
        self.assertEqual(ut._parse_color("hsl(0, 0%, 99%)"), (255, 255, 255))
        self.assertIsNone(ut._parse_color("hsl(1.2.3, 0%, 50%)"))
        for color in ("hsl(0,0%,93%)", "hsl(120deg, 50%, 96%)", "hsla(0 0% 90% / 1)"):
            with self.subTest(color):
                result = ut.sanitize_html('<p>Hi</p><span style="color:%s">%s</span>' % (color, C))
                self.assertNotIn(C, result.plain)
                self.assertIn("hidden_html_low_contrast", [f["vector"] for f in result.findings])
        for color in ("hsl(0,0%,20%)", "hsl(0,0%,50%)"):
            with self.subTest(color):
                self.assertIn(C, ut.sanitize_html('<p>Hi</p><span style="color:%s">%s</span>' % (color, C)).plain)
