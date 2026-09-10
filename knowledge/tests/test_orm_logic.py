# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.exceptions import AccessError, ValidationError
from psycopg2.errors import ForeignKeyViolation, RestrictViolation
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

        # Tests [@ANCHOR: manual_compute_breadcrumbs]

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
        # Note: Odoo's _assertRaises override in Odoo 19 does not correctly handle tuples of exceptions,
        # so we catch them manually to ensure compatibility and robustness.
        raised = False
        try:
            with self.env.cr.savepoint():
                self.article_a.unlink()
        except (ForeignKeyViolation, RestrictViolation):
            raised = True

        self.assertTrue(
            raised,
            "unlink() should have raised a RestrictViolation or ForeignKeyViolation",
        )

    def test_05_breadcrumb_excludes_archived_ancestor(self):
        # [@ANCHOR: test_manual_breadcrumb_archived_ancestor]

        # Tests [@ANCHOR: manual_compute_breadcrumbs]
        """
        Bug-hunt regression (2026-09-09): _compute_breadcrumb_article_ids's
        recursive ancestor lookup is raw SQL against the table directly, so
        it saw an archived (active=False) parent regardless of the
        `active` flag -- the compute method must re-derive which ancestor
        ids are actually visible (active + ir.rule) via a real search()
        before including them, or an archived parent's id (and, via the
        template, its name) keeps showing up in its still-active
        children's breadcrumb.
        """
        archived_parent = self.env["knowledge.article"].create(
            {"name": "Archived Parent", "is_published": True, "active": False}
        )
        child = self.env["knowledge.article"].create(
            {
                "name": "Active Child Of Archived",
                "is_published": True,
                "parent_id": archived_parent.id,
            }
        )
        self.assertNotIn(
            archived_parent.id,
            child.breadcrumb_article_ids.ids,
            "An archived ancestor must not appear in the breadcrumb.",
        )

    def test_06_breadcrumb_excludes_inaccessible_ancestor(self):
        # [@ANCHOR: test_manual_breadcrumb_inaccessible_ancestor]

        # Tests [@ANCHOR: manual_compute_breadcrumbs]
        """
        Bug-hunt regression (2026-09-09): a published child article whose
        parent chain includes a private, unpublished ancestor must not
        expose (or crash on) that ancestor's id in its own
        breadcrumb_article_ids when computed for a low-privilege viewer --
        rendering `breadcrumb_article_ids` used to dereference every
        ancestor id via the ORM regardless of the viewing user's own
        ir.rule access, raising AccessError for a public/portal visitor
        the instant an inaccessible ancestor's name was read.
        """
        public_user = self.env.ref("base.public_user")
        private_parent = self.env["knowledge.article"].create(
            {
                "name": "Private Root Notes",
                "is_published": False,
                "internal_permission": "none",
            }
        )
        published_child = self.env["knowledge.article"].create(
            {
                "name": "Published Child",
                "is_published": True,
                "parent_id": private_parent.id,
            }
        )
        child_as_public = published_child.with_user(public_user)
        try:
            breadcrumb_ids = child_as_public.breadcrumb_article_ids.ids
        except AccessError:
            self.fail(
                "Computing breadcrumb_article_ids must never raise for a "
                "viewer who can read the child itself, even when an "
                "ancestor is outside that viewer's own access."
            )
        self.assertNotIn(
            private_parent.id,
            breadcrumb_ids,
            "A private ancestor outside the viewer's access must not "
            "appear in the breadcrumb.",
        )


