# SPDX-License-Identifier: AGPL-3.0-or-later
# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
import uuid

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase



@tagged("standard", "post_install", "-at_install")
class TestLogAnalyzer(HamsTransactionCase):
    def test_01_log_analyzer_views(self):
        # Tests [@ANCHOR: test_log_analyzer_views]
        v1 = self.env["pager.log.pattern"].get_view(view_type="list")
        self.assertIn("regex", v1["arch"])

        v2 = self.env["pager.log.file"].get_view(view_type="list")
        self.assertIn("filepath", v2["arch"])

    def test_02_headless_api_translation(self):
        # Tests [@ANCHOR: pd_log_api_i18n]

        self.env["pager.log.pattern"].create({
            "name": "Dummy",
            "regex": ".*",
            "severity": "low",
        })
        recs = self.env["pager.log.pattern"].search([], limit=1)
        self.assertTrue(recs)

@tagged("standard", "post_install", "-at_install")
class TestLogAnalyzerReal(RealTransactionCase):
    def _register_log_file(self, filepath, **extra):
        log_file = self.env["pager.log.file"].create({"filepath": filepath, **extra})
        self.env.cr.commit()
        return log_file

    def _search(self, file_path):
        self.authenticate("admin", "admin")
        res = self.url_open(
            "/api/v1/pager/logs/search",
            json={"params": {"file_path": file_path, "regex_query": "test"}},
        )
        self.assertEqual(res.status_code, 200)
        return res.json()

    def test_03_async_bastion_pattern(self):
        """
        Verify that search_logs creates a pager.log.search.job and returns its UUID instead of blocking.
        """
        # Tests [@ANCHOR: pager_duty:search_logs]
        # search_logs() only accepts a path the caller's own scope has registered as an
        # active pager.log.file (the cross-tenant fix in 5b9606ac). This test predates that
        # fix and never registered one, so it had been failing on "not a registered
        # pager_duty log target" ever since. Register it the way an operator would.
        self._register_log_file("/var/log/syslog")
        self.authenticate("admin", "admin")
        res = self.url_open(
            "/api/v1/pager/logs/search",
            json={"params": {"file_path": "/var/log/syslog", "regex_query": "test"}}
        )
        self.assertEqual(res.status_code, 200)
        
        data = res.json().get("result", {})
        
        # Test env might not have Redis, so handle both valid IPC or expected missing-Redis error
        if "error" in data:
            self.assertTrue("Redis" in data["error"] or "IPC" in data["error"])
        else:
            self.assertIn("job_id", data)
            self.env.invalidate_all()
            with self.registry.cursor() as new_cr:
                new_env = self.env(cr=new_cr)
                job = new_env["pager.log.search.job"].search([("uuid", "=", data["job_id"])], limit=1)
                self.assertTrue(job)
                self.assertEqual(job.state, "pending")

    def test_04_unregistered_path_under_var_log_is_rejected(self):
        """
        Being under /var/log is not enough: a path nobody registered is refused before any job
        is created or any request reaches the log daemon.
        """
        before = self.env["pager.log.search.job"].search_count([])
        body = self._search("/var/log/hams_never_registered_by_anyone.log")
        self.assertNotIn("result", body)
        self.assertEqual(body["error"]["data"]["name"], "odoo.exceptions.AccessError")
        self.assertIn("not a registered pager_duty log target", body["error"]["data"]["message"])
        self.env.invalidate_all()
        self.assertEqual(self.env["pager.log.search.job"].search_count([]), before)

    def test_05_path_registered_for_another_website_is_rejected(self):
        """
        The cross-tenant case search_logs()'s allow-list exists for: another website registered
        this exact file, but the caller's pager_log_file_website_company_rule scope does not
        include that website, so the caller may not search it.
        """
        # Tests [@ANCHOR: pager_duty:search_logs]
        other_website = self.env["website"].create({"name": "Another tenant's website"})
        admin = self.env.ref("base.user_admin")
        self.assertNotEqual(admin.website_id, other_website)
        # The other tenant's own pager admin registers the file: the same rule that this test is
        # about stops admin from creating a pager.log.file for a website outside its own scope.
        other_admin = self.env["res.users"].create(
            {
                "name": "Other Tenant Pager Admin",
                "login": "other_tenant_pager_admin_%s" % uuid.uuid4().hex[:8],
                "website_id": other_website.id,
                # Portal, as in test_pager_security's website isolation test: Odoo refuses a
                # website on an internal user's partner.
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id, self.env.ref("pager_duty.group_pager_admin").id])],
            }
        )
        self.env["pager.log.file"].with_user(other_admin).create(
            {"filepath": "/var/log/another_tenant_app.log", "website_id": other_website.id}
        )
        self.env.cr.commit()
        body = self._search("/var/log/another_tenant_app.log")
        self.assertNotIn("result", body)
        self.assertEqual(body["error"]["data"]["name"], "odoo.exceptions.AccessError")
        self.assertIn("not a registered pager_duty log target", body["error"]["data"]["message"])
