# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""Refusal and fail-closed branches that no other test reached: every state of a log-search poll, the log
search with no Redis, a failed Redis write while counting a wrong domain-API secret, and the executable
provisioning RPC when binary_downloader is not installed."""
from unittest.mock import MagicMock

import redis as real_redis

from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests.common import tagged


@tagged("post_install", "-at_install")
class TestPagerRefusalBranches(HamsHttpCase):
    def _poll(self, job_id):
        response = self.url_open(
            "/api/v1/pager/logs/search_poll",
            json={"jsonrpc": "2.0", "method": "call", "params": {"job_id": job_id}},
        )
        return response.json()["result"]

    def _admin_login(self):
        self.env["res.users"].create(
            {
                "name": "Refusal Admin",
                "login": "refusal_admin",
                "password": "refusal_admin",
                "group_ids": [(6, 0, [self.env.ref("pager_duty.group_pager_admin").id])],
            }
        )
        self.env.flush_all()
        self.authenticate("refusal_admin", "refusal_admin")

    def test_poll_reports_each_job_state_without_inventing_a_result(self):
        # Tests [@ANCHOR: pager_duty:search_logs_poll]
        Job = self.env["pager.log.search.job"]
        Job.create({"uuid": "job-pending", "state": "pending"})
        Job.create({"uuid": "job-error", "state": "error", "result_payload": "daemon said no"})
        Job.create({"uuid": "job-error-empty", "state": "error"})
        Job.create({"uuid": "job-garbled", "state": "done", "result_payload": "{not json"})
        self._admin_login()
        self.assertEqual(self._poll("job-unknown"), {"error": "Job not found"})
        self.assertEqual(self._poll("job-pending"), {"status": "pending"})
        self.assertEqual(self._poll("job-error"), {"error": "daemon said no"})
        self.assertEqual(self._poll("job-error-empty"), {"error": "Daemon processing error"})
        self.assertEqual(self._poll("job-garbled"), {"error": "Invalid result payload"})

    def test_log_search_without_redis_is_refused_and_creates_no_job(self):
        # Tests [@ANCHOR: pager_duty:search_logs]
        self.env["pager.log.file"].create({"filepath": "/var/log/refusal_no_redis.log"})
        self._admin_login()
        self.safe_patch("odoo.addons.pager_duty.controllers.log_api.redis_pool", None)
        jobs_before = self.env["pager.log.search.job"].search_count([])
        response = self.url_open(
            "/api/v1/pager/logs/search",
            json={
                "jsonrpc": "2.0", "method": "call",
                "params": {"file_path": "/var/log/refusal_no_redis.log", "regex_query": "x"},
            },
        )
        self.assertIn("Redis not available", response.json()["result"]["error"])
        self.assertEqual(self.env["pager.log.search.job"].search_count([]), jobs_before)

    def test_a_redis_failure_while_counting_a_wrong_secret_still_refuses(self):
        # Tests [@ANCHOR: pager_duty:update_domains]
        self.env["ir.config_parameter"].set_param("pager_duty.domain_api_identity", "the-real-secret")
        mock_redis = self.safe_patch("odoo.addons.pager_duty.controllers.domain_api.redis")
        mock_redis.exceptions = real_redis.exceptions
        self.safe_patch("odoo.addons.pager_duty.controllers.domain_api.redis_pool", MagicMock())
        client = mock_redis.Redis.return_value
        client.get.return_value = None
        client.pipeline.return_value.execute.side_effect = real_redis.ConnectionError("down")
        response = self.url_open(
            "/api/v1/pager_duty/update_domains",
            json={
                "jsonrpc": "2.0", "method": "call",
                "params": {"domains": ["example.com"], "api_identity": "wrong-secret"},
            },
        )
        self.assertEqual(response.json()["result"]["status"], "error")
        self.assertIn("Unauthorized", response.json()["result"]["message"])
        self.assertFalse(self.env["pager.check"].search([("check_type", "=", "certbot")]))

    def test_provisioning_is_refused_when_binary_downloader_is_not_installed(self):
        # Tests [@ANCHOR: rpc_ensure_executable_security]
        utils = self.env["zero_sudo.security.utils"]
        self.safe_patch_object(type(utils), "_resolve_dependency_cycle", return_value=False)
        result = self.env["pager.check"].rpc_ensure_executable("ping")
        self.assertEqual(result["status"], "error")
        self.assertIn("not installed", result["message"])
