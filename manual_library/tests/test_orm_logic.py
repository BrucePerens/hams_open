# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.exceptions import ValidationError
from psycopg2.errors import RestrictViolation
from odoo.tools import mute_logger


@tagged("post_install", "-at_install")
class TestManualORMLogic(HamsTransactionCase):

    def setUp(self):
        super(TestManualORMLogic, self).setUp()
        self.article_a = self.env["knowledge.article"].create({"name": "Node A"})
        self.article_b = self.env["knowledge.article"].create(
            {"name": "Node B", "parent_id": self.article_a.id}
        )
        self.article_c = self.env["knowledge.article"].create(
            {"name": "Node C", "parent_id": self.article_b.id}
        )

    def test_01_prevent_circular_hierarchy(self):
        # [@ANCHOR: test_manual_check_hierarchy]
        # Tests [@ANCHOR: manual_check_hierarchy]
        # Tests [@ANCHOR: story_manual_hierarchy]
        # Tests [@ANCHOR: journey_admin_managing]
        """
        Verify the _check_hierarchy constraint prevents a parent from being nested
        under its own child, avoiding infinite recursion loops in the ORM/UI.
        """
        with self.assertRaises(
            ValidationError, msg="ORM must prevent circular references."
        ):
            # Attempt to set Node A's parent to Node C (A -> B -> C -> A)
            self.article_a.write({"parent_id": self.article_c.id})
            self.env.flush_all()
        # Tests [@ANCHOR: manual_compute_website_url]
        # Tests [@ANCHOR: story_manual_url_generation]

    def test_02_url_slug_generation(self):
        # [@ANCHOR: test_manual_url_slug_generation]
        """
        Verify that the custom compute method generates safe, URL-friendly slugs
        appended to the article ID to ensure uniqueness.
        """
        complex_article = self.env["knowledge.article"].create(
            {"name": "API Documentation v2.0 (Alpha)!"}
        )

        # FIXED: Removed the trailing hyphen as .strip('-') correctly cleans it
        expected_slug = f"/manual/{complex_article.id}-api-documentation-v2-0-alpha"

        self.assertEqual(
            complex_article.website_url,
            expected_slug,
            "The _compute_website_url method must generate a clean, safe slug.",
        )

    def test_03_url_slug_empty_name(self):
        """Verify the compute method does not crash if name is temporarily empty."""
        empty_article = self.env["knowledge.article"].create({"name": "Temp"})
        empty_article.name = False

        self.assertEqual(
            empty_article.website_url,
            f"/manual/{empty_article.id}-",
            "Slug generation must degrade gracefully with an empty name.",
        )

    @mute_logger("odoo.sql_db")
    def test_04_parent_deletion_restriction(self):
        """
        Verify that parent articles cannot be deleted if they have children,
        due to the ondelete='restrict' configuration.
        """
        with self.assertRaises(RestrictViolation):
            with self.env.cr.savepoint():
                self.article_a.unlink()
