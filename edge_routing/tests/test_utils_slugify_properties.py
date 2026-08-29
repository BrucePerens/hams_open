# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0
"""Property-based tests for edge_routing.utils.slugify(), per
docs/proposals/CODE_REVIEW_PROCESS.md's "Formal verification tooling"
section -- same pattern already applied to callsign_validation.py,
ham.geo.utils's grid math, and other small, pure, well-typed functions.
slugify() takes a plain string and returns a plain string, no self.env/DB
access, so it is tested directly with BaseCase (matching test_utils.py's
own existing example-based tests) rather than a transactional case.
"""
from hypothesis import given, settings, strategies as st

from odoo.tests.common import BaseCase
from odoo.tests import tagged
from odoo.addons.edge_routing.utils import slugify

_SETTINGS = settings(max_examples=200, deadline=None)

# Printable text, including non-ASCII, matching the kind of user-entered
# titles slugify() is actually fed (page/group names, not raw bytes).
_TEXT = st.text(min_size=0, max_size=200)


@tagged("post_install", "-at_install")
class TestSlugifyProperties(BaseCase):
    # Tests [@ANCHOR: edge_routing:utils_slugify]

    @_SETTINGS
    @given(_TEXT)
    def test_output_is_always_url_safe(self, s):
        result = slugify(s)
        for ch in result:
            self.assertTrue(
                ch == "-" or ch.isascii() and ch.isalnum() and ch.islower() or ch.isdigit(),
                f"unsafe character {ch!r} in slug for input {s!r}: {result!r}",
            )

    @_SETTINGS
    @given(_TEXT)
    def test_never_starts_or_ends_with_a_hyphen(self, s):
        result = slugify(s)
        if result:
            self.assertNotEqual(result[0], "-")
            self.assertNotEqual(result[-1], "-")

    @_SETTINGS
    @given(_TEXT)
    def test_idempotent(self, s):
        once = slugify(s)
        twice = slugify(once)
        self.assertEqual(once, twice)

    @_SETTINGS
    @given(_TEXT, st.integers(min_value=1, max_value=200))
    def test_result_never_exceeds_a_positive_max_length(self, s, max_length):
        result = slugify(s, max_length=max_length)
        self.assertLessEqual(len(result), max_length)

    @_SETTINGS
    @given(_TEXT)
    def test_max_length_zero_yields_an_empty_string(self, s):
        # max_length=0 is a degenerate but legal call ("truncate to nothing"
        # is a coherent request from a caller building a slug length budget
        # dynamically) -- the result must respect it like any other
        # non-negative max_length, not silently ignore it.
        self.assertEqual(slugify(s, max_length=0), "")
