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
import time

from odoo.tests.common import BaseCase
from odoo.tools import html_sanitize


class TestSvgReviewFindings(BaseCase):

    def test_01_many_sibling_svgs_are_sanitised_in_roughly_linear_time(self):
        # `_replace_with_text` rebuilds `parent.text` with `+=` for every adjacent svg,
        # so the cost is quadratic: 20000 empty blocks (220 KB) take about 11 s here,
        # 330 KB about 33 s, against 0.1 s unpatched. Any user who can write an Html
        # field can pin a worker.
        value = "<svg></svg>" * 20000
        started = time.monotonic()
        html_sanitize(value)
        elapsed = time.monotonic() - started
        self.assertLess(elapsed, 2.0, f"20000 sibling svgs took {elapsed:.1f}s")

    def test_02_many_svgs_interleaved_with_elements_are_linear_too(self):
        value = "<p>" + "<svg></svg><b>x</b>" * 20000 + "</p>"
        started = time.monotonic()
        html_sanitize(value)
        elapsed = time.monotonic() - started
        self.assertLess(elapsed, 3.0, f"20000 interleaved svgs took {elapsed:.1f}s")

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
