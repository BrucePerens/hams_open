# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Adversarial review of the SVG allowlist sanitizer: tests that FAIL today.

Each test demonstrates a confirmed problem found on night-shift/svg-allowlist.
They are expected to fail until the sanitizer is fixed; none changes the sanitizer.
"""
import re

from odoo.tests.common import BaseCase, tagged
from odoo.tools import html_sanitize

from .test_svg_sanitizer import assert_linear_cpu


@tagged("post_install", "-at_install")
class TestSvgReviewFindings(BaseCase):

    def test_01_many_sibling_svgs_are_sanitised_in_roughly_linear_time(self):
        # `_replace_with_text` rebuilds `parent.text` with `+=` for every adjacent svg,
        # so the cost is quadratic: 20000 empty blocks (220 KB) take about 11 s here,
        # 330 KB about 33 s, against 0.1 s unpatched. Any user who can write an Html
        # field can pin a worker. Linear means doubling the input about doubles the CPU time
        # (a quadratic cost quadruples it); see assert_linear_cpu for why this is not a wall-clock budget.
        assert_linear_cpu(self, lambda n: "<svg></svg>" * n, 10000, 20000, "sibling svgs")

    def test_02_many_svgs_interleaved_with_elements_are_linear_too(self):
        # The same cost with an element between the blocks: `<svg></svg><b>x</b>` repeated, inside one
        # paragraph, so every block has a different neighbour than in test_01 and the fix cannot be a
        # special case for adjacent siblings. Doubling 10000 to 20000 pairs must about double the CPU time.
        assert_linear_cpu(
            self, lambda n: "<p>" + "<svg></svg><b>x</b>" * n + "</p>", 10000, 20000, "interleaved svgs"
        )

    def test_03_style_cannot_move_or_size_an_svg_over_the_page(self):
        # The style whitelist accepts negative margins and unbounded width/height. An
        # svg is a replaced element, so unlike a div it paints above neighbouring text
        # and a transparent rect inside it takes the clicks: a UI-redress overlay in
        # the site's origin (headless Chromium: elementFromPoint over a link returns
        # the rect). It survives sanitize_style=True fields too, where a div with the
        # same style does not cover the link.
        value = (
            '<svg style="margin-left:-5000px;margin-top:-5000px;'
            'width:20000px;height:20000px">'
            '<rect width="100%" height="100%" fill="transparent"/></svg>'
        )
        for options in ({}, {"sanitize_style": True}):
            result = str(html_sanitize(value, **options))
            match = re.search(r'style="([^"]*)"', result)
            style = match.group(1) if match else ""
            self.assertNotRegex(style, r":\s*-", f"negative length survived: {style!r}")
            self.assertNotRegex(
                style, r"(?:width|height):\s*\d{4,}", f"huge size survived: {style!r}"
            )

    def test_04_class_utilities_cannot_pin_an_svg_over_the_viewport(self):
        # strip_style fields drop `style`, but `class` is kept, so Bootstrap utilities
        # still position the svg: class="position-fixed top-0 start-0 w-100 h-100"
        # covered the page in headless Chromium with the site's bootstrap.css. Same as
        # for HTML elements today (pre-existing); the SVG block should not widen it.
        value = (
            '<svg class="position-fixed top-0 start-0 w-100 h-100">'
            '<rect width="100%" height="100%" fill="transparent"/></svg>'
        )
        result = str(html_sanitize(value, strip_style=True))
        self.assertNotIn("position-fixed", result)
