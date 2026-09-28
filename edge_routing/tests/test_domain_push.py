# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged


@tagged("post_install", "-at_install")
class TestDomainPush(HamsTransactionCase):

    def setUp(self):
        super().setUp()
        # cloudflare _inherit's edge.routing.domain and auto-provisions a
        # real Cloudflare custom hostname on create(), requiring a
        # matching website.domain record to exist -- unrelated to what
        # this file tests (pager_duty push logic/batching), so neutralize
        # it rather than fabricating 1000+ website fixtures the actual
        # feature under test doesn't need. Guarded so this file still
        # works if cloudflare (not an edge_routing dependency) isn't
        # installed -- checked via ir.module.module rather than hasattr()
        # probing the class, since hasattr() is banned for masking
        # architectural type uncertainty (matches ham_base/tests/
        # test_config_parameter_security.py's own established pattern
        # for the identical "optional module installed in this combined
        # test run" situation).
        acl_svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "zero_sudo.odoo_facility_service_internal"
        )
        cloudflare_installed = bool(
            self.env["ir.module.module"]
            .with_user(acl_svc_uid)
            .search([("name", "=", "cloudflare"), ("state", "=", "installed")], limit=1)
        )
        if cloudflare_installed:
            domain_cls = type(self.env["edge.routing.domain"])
            self.safe_patch_object(
                domain_cls, "_create_cloudflare_custom_hostname_batch"
            )

    def test_domain_push_logic(self):
        """Test the logic that triggers the PagerDuty sync cron."""
        # Instead of dealing with postcommit complexities in tests,
        # we will directly test the _trigger_pager_duty_sync logic by simulating the environment.
        domain_model = self.env["edge.routing.domain"].with_user(
            self.env.ref("base.user_admin")
        )

        # Create a domain
        domain_model.create(
            {
                "name": "manualpush.com",
                "target_slug": "manualpush",
            }
        )
        self.env.flush_all()

        # We can't easily mock the inner function `push_to_pager_duty`
        # But we can call the outer function to ensure it doesn't crash.
        domain_model._trigger_pager_duty_sync()

    # The value itself is irrelevant to what these tests prove (the endpoint's
    # own hmac.compare_digest is exercised by pager_duty's own suite); what
    # matters is that push_all_to_pager_duty actually transmits whatever is
    # configured. Never a real secret -- this is a per-test fixture value.
    TEST_API_IDENTITY = "unit-test-domain-api-identity"

    def _configure_api_identity(self, value=TEST_API_IDENTITY):
        self.env["ir.config_parameter"].set_param(
            "pager_duty.domain_api_identity", value
        )

    def _mock_post_accepted(self):
        """Patches requests.post with a response shaped like the real
        `type="jsonrpc"` route's own success envelope. Without this, a bare
        MagicMock's `.json()` returns another MagicMock, which the new
        response-body check below would (correctly) treat as a refusal."""
        mock_post = self.safe_patch(
            "odoo.addons.edge_routing.models.domain.requests.post"
        )
        mock_post.return_value.status_code = 200
        mock_post.return_value.json.return_value = {
            "jsonrpc": "2.0",
            "id": None,
            "result": {"status": "success"},
        }
        return mock_post

    def test_push_all_to_pager_duty_batching(self):
        # Tests [@ANCHOR: edge_routing:COMM_domain_push_pagerduty]

        domain_model = self.env["edge.routing.domain"].with_user(
            self.env.ref("base.user_admin")
        )

        # Create 1005 domains
        vals_list = [{'name': f'domain{i}.com', 'target_slug': f'target{i}'} for i in range(1005)]
        domain_model.create(vals_list)

        self._configure_api_identity()
        mock_post = self._mock_post_accepted()
        domain_model.push_all_to_pager_duty()

        total_domains = domain_model.search_count([])
        self.assertGreaterEqual(total_domains, 1005)

        mock_post.assert_called_once()
        _, kwargs = mock_post.call_args
        posted_domains = kwargs.get('json', {}).get('params', {}).get('domains', [])
        self.assertEqual(len(posted_domains), total_domains)

    def test_push_all_to_pager_duty_sends_the_configured_api_identity(self):
        # Tests [@ANCHOR: edge_routing:COMM_domain_push_pagerduty]
        # Regression for the bug-hunt finding of 2026-09-27: this POST used to
        # carry only {"domains": [...]} and no `api_identity` at all, so
        # pager_duty's own auth="public" + hmac.compare_digest gate
        # (the pager_duty:update_domains anchor) refused every single sync.
        # The refusal arrives inside an HTTP 200 JSON-RPC envelope, so neither
        # raise_for_status() nor the surrounding "don't fail the cron" except
        # noticed -- the domain list silently never reached
        # pager.check.update_lets_encrypt_domains(). test_push_all_to_pager_
        # duty_batching above could not catch this: it asserts only the
        # `domains` key of a fully mocked requests.post, which is identical
        # whether or not the receiving endpoint would accept the call.
        domain_model = self.env["edge.routing.domain"].with_user(
            self.env.ref("base.user_admin")
        )
        domain_model.create({"name": "identity-check.com", "target_slug": "identity-check"})

        self._configure_api_identity()
        mock_post = self._mock_post_accepted()
        domain_model.push_all_to_pager_duty()

        mock_post.assert_called_once()
        _, kwargs = mock_post.call_args
        params = kwargs.get("json", {}).get("params", {})
        self.assertEqual(
            params.get("api_identity"),
            self.TEST_API_IDENTITY,
            "the push must carry the configured pager_duty.domain_api_identity, "
            "or /api/v1/pager_duty/update_domains refuses it as Unauthorized",
        )

    def test_push_all_to_pager_duty_is_skipped_when_no_identity_is_configured(self):
        # Tests [@ANCHOR: edge_routing:COMM_domain_push_pagerduty]
        # With no shared secret configured the endpoint is guaranteed to
        # refuse, and every refusal increments its own per-source-IP
        # failed-attempt counter (MAX_FAILED_ATTEMPTS=10 per 60s) -- so the
        # cron must not POST at all rather than lock this origin out of it.
        domain_model = self.env["edge.routing.domain"].with_user(
            self.env.ref("base.user_admin")
        )
        self._configure_api_identity(False)
        mock_post = self._mock_post_accepted()

        with self.assertLogs(
            "odoo.addons.edge_routing.models.domain", level="WARNING"
        ) as logs:
            domain_model.push_all_to_pager_duty()

        mock_post.assert_not_called()
        self.assertTrue(
            any("domain_api_identity" in line for line in logs.output),
            "the skip must say why, not be silent",
        )

    def test_push_all_to_pager_duty_logs_a_refusal_returned_in_a_200_body(self):
        # Tests [@ANCHOR: edge_routing:COMM_domain_push_pagerduty]
        # The discriminating half of the regression: a `type="jsonrpc"` route
        # signals an application-level refusal with HTTP 200 and the refusal
        # in the body, so raise_for_status() alone can never detect it. If the
        # body check is ever removed, this test fails.
        domain_model = self.env["edge.routing.domain"].with_user(
            self.env.ref("base.user_admin")
        )
        self._configure_api_identity()
        mock_post = self._mock_post_accepted()
        mock_post.return_value.json.return_value = {
            "jsonrpc": "2.0",
            "id": None,
            "result": {"status": "error", "message": "Unauthorized"},
        }

        with self.assertLogs(
            "odoo.addons.edge_routing.models.domain", level="WARNING"
        ) as logs:
            domain_model.push_all_to_pager_duty()

        self.assertTrue(
            any("Unauthorized" in line for line in logs.output),
            "a refusal returned inside a 200 body must still be logged, not "
            "mistaken for a successful sync",
        )

    def test_cron_actually_runs_through_the_real_scheduler_path(self):
        # Tests [@ANCHOR: edge_routing_push_pager_duty_cron_runs]
        # test_push_all_to_pager_duty_batching above calls
        # push_all_to_pager_duty() directly, bypassing the real execution
        # path ir.cron actually uses: ir.actions.server.run(), which calls
        # _can_execute_action_on_records() first and requires WRITE access
        # to the cron's declared model_id (edge.routing.domain) before
        # running anything. ir.model.access.csv deliberately keeps this
        # service account read-only on that model (push_all_to_pager_duty()
        # only ever reads domain rows to push externally, never writes them
        # back) -- found live: the cron 500'd/failed with "Forbidden server
        # action" on every scheduled run until group_ids was set on the
        # action itself, authorizing it without broadening the model's
        # actual write ACL.
        self._configure_api_identity()
        mock_post = self._mock_post_accepted()
        cron = self.env.ref("edge_routing.ir_cron_push_pager_duty")
        cron.ir_actions_server_id.with_user(cron.user_id.id).run()
        mock_post.assert_called_once()
