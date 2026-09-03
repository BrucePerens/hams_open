# SPDX-License-Identifier: AGPL-3.0-or-later
# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.exceptions import AccessError
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestLogSearchJobSecurity(HamsTransactionCase):
    # Adversarial security review, 2026-09-03: ir.model.access.csv granted
    # base.group_user (every internal user, not just pager_duty.
    # group_pager_admin) full CRUD on pager.log.search.job, with only
    # group_pager_admin and group_pager_service having a matching ir.rule
    # -- any plain base.group_user caller got ZERO record-level
    # restriction, able to read every admin log-search's result_payload
    # or forge/delete jobs via direct RPC. rpc_update_state() also had no
    # caller-identity check at all.
    def setUp(self):
        super().setUp()
        self.plain_internal_user = self.env["res.users"].create(
            {
                "name": "Plain Internal User",
                "login": "log_search_job_plain_user",
                "lang": "en_US",
                "group_ids": [(6, 0, [self.env.ref("base.group_user").id])],
            }
        )
        self.pager_admin = self.env["res.users"].create(
            {
                "name": "Pager Admin",
                "login": "log_search_job_pager_admin",
                "lang": "en_US",
                "group_ids": [
                    (
                        6,
                        0,
                        [
                            self.env.ref("pager_duty.group_pager_admin").id,
                            self.env.ref("base.group_portal").id,
                        ],
                    )
                ],
            }
        )
        self.job = self.env["pager.log.search.job"].create(
            {"uuid": "test-uuid-1234", "state": "pending"}
        )

    def test_a_plain_internal_user_cannot_read_log_search_jobs(self):
        # The ACL row itself was narrowed from base.group_user to
        # pager_duty.group_pager_admin, so a plain internal user now
        # fails closed at the model-access-check level (AccessError)
        # rather than merely getting an ir.rule-filtered empty result.
        with self.assertRaises(
            AccessError,
            msg="[!] DIAGNOSTIC FOR AI: a plain base.group_user caller "
            "must not be able to read admin log-search job results.",
        ):
            self.env["pager.log.search.job"].with_user(
                self.plain_internal_user
            ).search([("id", "=", self.job.id)])

    def test_a_plain_internal_user_cannot_write_or_unlink_a_job(self):
        with self.assertRaises(AccessError):
            self.job.with_user(self.plain_internal_user).write({"state": "done"})
        with self.assertRaises(AccessError):
            self.job.with_user(self.plain_internal_user).unlink()

    def test_a_pager_admin_can_still_read_jobs(self):
        self.assertTrue(
            self.env["pager.log.search.job"]
            .with_user(self.pager_admin)
            .search([("id", "=", self.job.id)]),
            "a real pager admin must still be able to read their own jobs",
        )

    def test_rpc_update_state_rejects_a_non_service_caller(self):
        with self.assertRaises(
            AccessError,
            msg="[!] DIAGNOSTIC FOR AI: a non-service caller must not be "
            "able to feed forged results into a log-search job via "
            "rpc_update_state().",
        ):
            self.env["pager.log.search.job"].with_user(
                self.plain_internal_user
            ).rpc_update_state("test-uuid-1234", "done", '{"forged": true}')
        self.job.invalidate_recordset()
        self.assertEqual(self.job.state, "pending")

    def test_rpc_update_state_works_for_the_real_service_account(self):
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "pager_duty.user_pager_service_internal"
        )
        svc_user = self.env["res.users"].browse(svc_uid)
        result = self.env["pager.log.search.job"].with_user(svc_user).rpc_update_state(
            "test-uuid-1234", "done", '{"lines": []}'
        )
        self.assertTrue(result)
        self.job.invalidate_recordset()
        self.assertEqual(self.job.state, "done")
