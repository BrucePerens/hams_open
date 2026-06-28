# -*- coding: utf-8 -*-
import odoo
from odoo.tests import tagged
from odoo.exceptions import ValidationError
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
import json


@tagged("post_install", "-at_install")
class TestRobustnessAndBoundaries(HamsHttpCase):

    def setUp(self):
        super().setUp()
        self.user_test = self.env["res.users"].create(
            {
                "name": "Robust User",
                "login": "robustuser",
                "email": "robust@example.com",
                "website_slug": "robustuser",
                "group_ids": [
                    (
                        6,
                        0,
                        [
                            self.env.ref("base.group_portal").id,
                            self.env.ref("user_websites.group_user_websites_user").id,
                        ],
                    )
                ],
            }
        )

    def test_01_slug_generation_exhaustion(self):
        """Verify that if the slug namespace is completely exhausted (1000 retries), it raises a ValidationError."""
        # We mock the collision check to always return True (simulating a permanent collision)
        self.safe_patch(
            "odoo.addons.edge_routing.models.routing_mixin.EdgeRoutingMixin._check_slug_collision",
            return_value=True,
        )
        with self.assertRaises(
            ValidationError,
            msg="Must raise ValidationError if 1000 slug variations are exhausted.",
        ):
            self.env["res.users"].create(
                {
                    "name": "Infinite Loop",
                    "login": "infloop",
                }
            )
            self.env.flush_all()

    def test_02_uppercase_reserved_slug(self):
        """Verify that trying to use a reserved slug with mixed casing is caught by the validations."""
        with self.assertRaises(
            ValidationError,
            msg="Mixed-case reserved slugs must be blocked after slugification.",
        ):
            self.env["res.users"].create(
                {
                    "name": "ConTacTUs",
                    "login": "contactustest",
                    "website_slug": "ConTacTUs",
                }
            )
            self.env.flush_all()

    def test_03_violation_report_length_truncation(self):
        """Verify that overly long descriptions are safely truncated without crashing the DB."""
        self.authenticate(None, None)

        long_desc = "A" * 6000

        response = self.url_open(
            "/website/report_violation",  # burn-ignore-route
            data={
                "csrf_token": odoo.http.Request.csrf_token(self),
                "url": f"/{self.user_test.website_slug}/home",
                "description": long_desc,
                "email": "truncatetest@example.com",
            },
            method="POST",
        )

        self.assertEqual(response.status_code, 200)

        report = self.env["content.violation.report"].search(
            [("reported_by_email", "=", "truncatetest@example.com")], limit=1
        )
        self.assertTrue(report, "The report must be successfully created.")
        self.assertEqual(
            len(report.description),
            5000,
            "The controller must truncate the string to exactly 5000 characters.",
        )

    def test_04_gdpr_export_empty_state_json_validity(self):
        """Verify that the custom JSON streaming generator produces valid JSON when the user has 0 records."""
        self.authenticate(self.user_test.login, self.user_test.login)

        response = self.url_open(
            "/my/privacy/export",
            data={"csrf_token": odoo.http.Request.csrf_token(self)},
            method="POST",
        )

        self.assertEqual(response.status_code, 200)

        try:
            data = json.loads(response.content)
        except json.JSONDecodeError:
            self.fail(
                "GDPR Export generated invalid JSON for an empty user state. Check generator trailing commas."
            )

        self.assertEqual(len(data["pages"]), 0)
        self.assertEqual(len(data["blog_posts"]), 0)

    def test_05_gdpr_export_streaming_json_escaping(self):
        """
        Verify that the custom JSON streaming generator safely escapes
        dangerous characters (newlines, quotes, backslashes) in user content.
        """
        self.authenticate(self.user_test.login, self.user_test.login)

        # Create content with aggressive string breaks
        malicious_html = "<p>He said, \"Hello\nWorld!\" &amp; 'test'</p>"
        self.env["website.page"].create(
            {
                "url": f"/{self.user_test.website_slug}/nasty",
                "name": "Nasty Page",
                "type": "qweb",
                "arch": malicious_html,
                "owner_user_id": self.user_test.id,
            }
        )

        response = self.url_open(
            "/my/privacy/export",
            data={"csrf_token": odoo.http.Request.csrf_token(self)},
            method="POST",
        )

        self.assertEqual(response.status_code, 200)

        try:
            data = json.loads(response.content)
            self.assertEqual(
                data["pages"][0]["content"],
                malicious_html,
                "The JSON parser must successfully decode the escaped payload exactly as it was inputted.",
            )
        except json.JSONDecodeError as e:
            self.fail(
                f"Custom JSON generator produced invalid JSON due to character escaping failure: {e}"
            )
