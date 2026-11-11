# Copyright © Bruce Perens K6BP. AGPL-3.0.
"""Named CSS colours in the hidden-text check of models/untrusted_text.py."""
from odoo.tests.common import BaseCase, tagged

from odoo.addons.hams_helpdesk.models import untrusted_text as ut

C = "CANARY7731"


@tagged("post_install", "-at_install")
class TestUntrustedTextNamedColours(BaseCase):
    def test_pale_named_colours_on_a_white_page_are_hidden_text(self):
        """`honeydew`, `lemonchiffon`, `cornsilk`, `lightcyan` and the other near-white names were missing from the colour
        table, so text in them (contrast 1.04 to 1.1 on white) passed with no finding while `white` and `ivory` were caught."""
        for name in ("honeydew", "lemonchiffon", "cornsilk", "lightcyan", "oldlace", "lavenderblush", "pink", "wheat"):
            with self.subTest(name):
                result = ut.sanitize_html('<p>Hi</p><span style="color:%s">%s</span>' % (name, C))
                self.assertNotIn(C, result.plain)
                self.assertIn("hidden_html_low_contrast", [f["vector"] for f in result.findings])
        self.assertEqual(ut._parse_color("HoneyDew"), (240, 255, 240))

    def test_dark_named_colours_on_a_black_background_are_hidden_text(self):
        html = '<div style="background-color:black"><span style="color:%s">%s</span></div>'
        for name in ("midnightblue", "darkblue"):
            with self.subTest(name):
                self.assertNotIn(C, ut.sanitize_html(html % (name, C)).plain)

    def test_readable_named_colours_are_kept(self):
        for name in ("red", "navy", "black", "gray", "orange"):
            with self.subTest(name):
                self.assertIn(C, ut.sanitize_html('<p>Hi</p><span style="color:%s">%s</span>' % (name, C)).plain)
