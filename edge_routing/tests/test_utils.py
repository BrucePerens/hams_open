# -*- coding: utf-8 -*-
from odoo.tests.common import BaseCase
from odoo.tests import tagged
from odoo.addons.edge_routing.utils import slugify


@tagged("post_install", "-at_install")
class TestUtils(BaseCase):
    # [@ANCHOR: edge_routing:test_utils_slugify]
    # Tests [@ANCHOR: edge_routing:utils_slugify]
    """
    Exhaustive unit tests for the custom slugify utility function to ensure
    URL safety regardless of user input.
    """

    def test_01_standard_slugify(self):
        self.assertEqual(slugify("Hello World"), "hello-world")
        self.assertEqual(slugify("My Test Page"), "my-test-page")

    def test_02_special_characters(self):
        self.assertEqual(slugify("C@f# é & R*staurant!"), "c-f-e-r-staurant")
        self.assertEqual(slugify("Hello_World-2026"), "hello-world-2026")

    def test_03_unicode_normalization(self):
        self.assertEqual(slugify("München"), "munchen")
        self.assertEqual(slugify("résumé"), "resume")

    def test_04_empty_and_null_inputs(self):
        self.assertEqual(slugify(None), "")
        self.assertEqual(slugify(""), "")
        self.assertEqual(slugify("   "), "")

    def test_05_max_length_truncation(self):
        long_string = "This is a very long string that should be truncated"
        # 10 chars: "this-is-a-" -> rstrip('-') -> "this-is-a"
        self.assertEqual(slugify(long_string, max_length=10), "this-is-a")
        self.assertEqual(slugify(long_string, max_length=5), "this")

    def test_06_leading_and_trailing_hyphens(self):
        self.assertEqual(slugify("---Test---"), "test")
        self.assertEqual(slugify("!@#Test!@#"), "test")
