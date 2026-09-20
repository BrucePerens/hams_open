# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.exceptions import ValidationError
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase

MALICIOUS = '<t name="X"><script>alert(1)</script></t>'


@tagged("post_install", "-at_install")
class TestViolationBatchWrite(RealTransactionCase):
    """Strike attribution for a website.page.write() carrying a stripped-malicious arch.

    Tests [@ANCHOR: user_websites:COMM_trigger_malicious_arch_violation]
    Bruce's decision: one strike per distinct owner/group in a batch; a
    multi-owner batch with a modified arch is refused by write().
    """

    def _tag(self):
        return self.id().rsplit(".", 1)[-1].replace("_", "-")

    def _user(self, name):
        return self.env["res.users"].create(
            {
                "name": name,
                "login": f"{name}_{self.id()}".lower().replace(" ", "_"),
                "email": f"{name}@example.com".replace(" ", "_"),
                "website_slug": f"{name}-{self._tag()}".lower().replace(" ", "-"),
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

    def _page(self, user, path, group=None):
        vals = {
            "url": f"/{user.website_slug}/{path}",
            "name": path,
            "type": "qweb",
            "arch": '<t name="ok"><div>ok</div></t>',
        }
        if group:
            vals["url"] = f"/{group.website_slug}/{path}"
            vals["user_websites_group_id"] = group.id
        else:
            vals["owner_user_id"] = user.id
        return self.env["website.page"].with_user(user).create(vals)

    def _strikes(self, rec):
        rec.invalidate_recordset(["violation_strike_count"])
        return rec.violation_strike_count

    def setUp(self):
        super().setUp()
        self.owner_a = self._user("Owner A")
        self.owner_b = self._user("Owner B")

    def test_01_single_record_write_one_strike_unchanged(self):
        page = self._page(self.owner_a, "single")
        before = self._strikes(self.owner_a)
        page.with_user(self.owner_a).write({"arch": MALICIOUS})
        self.assertEqual(self._strikes(self.owner_a), before + 1)
        report = self.env["content.violation.report"].search(
            [("target_url", "=", page.url)]
        )
        self.assertEqual(report.content_owner_id, self.owner_a)
        self.assertEqual(report.reported_by_user_id, self.owner_a)

    def test_02_one_owner_batch_costs_one_strike(self):
        pages = (
            self._page(self.owner_a, "b1")
            | self._page(self.owner_a, "b2")
            | self._page(self.owner_a, "b3")
        )
        before = self._strikes(self.owner_a)
        pages.with_user(self.owner_a).write({"arch": MALICIOUS})
        self.assertEqual(self._strikes(self.owner_a), before + 1)
        reports = self.env["content.violation.report"].search(
            [("target_url", "in", pages.mapped("url"))]
        )
        self.assertEqual(len(reports), 1)
        self.assertEqual(reports.target_url, pages[0].url)

    def test_03_two_owner_helper_strikes_each_owner_once(self):
        """The attribution helper itself strikes every distinct owner once."""
        a1, a2 = self._page(self.owner_a, "m1"), self._page(self.owner_a, "m2")
        b1 = self._page(self.owner_b, "m1")
        ba, bb = self._strikes(self.owner_a), self._strikes(self.owner_b)
        self.env["website.page"].with_user(self.owner_a)._trigger_malicious_arch_violation(
            {}, records=a1 | b1 | a2
        )
        self.assertEqual(self._strikes(self.owner_a), ba + 1)
        self.assertEqual(self._strikes(self.owner_b), bb + 1)
        self.assertEqual(
            self.env["content.violation.report"].search_count(
                [("target_url", "in", [a1.url, a2.url, b1.url])]
            ),
            2,
        )

    def test_04_multi_owner_batch_with_modified_arch_refused(self):
        group = self.env["user.websites.group"].create(
            {
                "name": f"Batch Group {self.id()}",
                "website_slug": f"batch-group-{self._tag()}",
            }
        )
        group.odoo_group_id.user_ids = [(4, self.owner_a.id)]
        own = self._page(self.owner_a, "own")
        grp = self._page(self.owner_a, "grp", group=group)
        before = self._strikes(self.owner_a)
        with self.assertRaises(ValidationError):
            (own | grp).with_user(self.owner_a).write({"arch": MALICIOUS})
            self.env.flush_all()
        self.assertNotIn("<script>", own.arch)
        # Benign multi-owner batch (arch not modified by the sanitizer) is untouched.
        (own | grp).with_user(self.owner_a).write({"name": "renamed"})
        self.assertEqual(self._strikes(self.owner_a), before)

    def test_05_editor_not_struck_group_and_reporter_attribution(self):
        group = self.env["user.websites.group"].create(
            {
                "name": f"Editor Group {self.id()}",
                "website_slug": f"editor-group-{self._tag()}",
            }
        )
        group.odoo_group_id.user_ids = [(4, self.owner_a.id)]
        gpage = self._page(self.owner_a, "gp", group=group)
        editor_before = self._strikes(self.owner_a)
        group_before = self._strikes(group)
        gpage.with_user(self.owner_a).write({"arch": MALICIOUS})
        self.assertEqual(self._strikes(group), group_before + 1)
        self.assertEqual(self._strikes(self.owner_a), editor_before)
        report = self.env["content.violation.report"].search(
            [("target_url", "=", gpage.url)]
        )
        self.assertEqual(report.content_group_id, group)
        self.assertFalse(report.content_owner_id)
        self.assertEqual(report.reported_by_user_id, self.owner_a)
