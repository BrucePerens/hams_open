# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from cryptography.fernet import Fernet
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestTunnelProvisioningSecurity(HamsTransactionCase):
    def test_service_account_can_read_and_write_tunnel_provisioned_flag(self):
        """
        action_ensure_tunnel_running() used to read/write
        cloudflare.tunnel.provisioned via .sudo(), which is forbidden on
        this platform (it bypasses ir.config_parameter's own ACL rather
        than switching to a properly scoped identity, and was never caught
        because is_odoo_module detection in a one-off manual lint
        invocation missed it -- the real repo-wide lint run does catch it).
        Fixed to route through the service-account architecture instead
        (cloudflare.user_cloudflare_tunnel via
        zero_sudo.security.utils._get_system_param/_set_system_param).
        Prove that round trip actually works for the service account, not
        just that it doesn't call .sudo().
        """
        utils = self.env["zero_sudo.security.utils"]
        key = "cloudflare.tunnel.provisioned"
        self.assertIn(key, utils._get_param_read_whitelist())
        self.assertIn(key, utils._get_param_write_whitelist())

        svc_uid = utils._get_service_uid("cloudflare.user_cloudflare_tunnel")
        self.assertTrue(
            self.env["res.users"].browse(svc_uid).is_service_account,
            "test setup assumption: this must actually be a service account",
        )

        utils_as_svc = utils.with_user(svc_uid)
        utils_as_svc._set_system_param(key, "True")
        self.env.registry.clear_cache()
        self.assertEqual(
            utils_as_svc._get_system_param(key),
            "True",
            "The tunnel service account MUST be able to read back the "
            "provisioned flag it just wrote, without an AccessError.",
        )

    def test_push_configuration_failure_does_not_block_daemon_start(self):
        # Tests [@ANCHOR: COMM_cloudflare_tunnel_push_config_catch_all]
        """
        action_ensure_tunnel_running() wraps action_push_configuration() in
        a broad 'except Exception' -- a Cloudflare API failure while
        pushing routes must not prevent the tunnel daemon itself from
        starting (SSH/basic connectivity should stay up while route
        provisioning retries later), and a failed push must not mark
        'provisioned' True (or it would never retry). Prove both halves of
        that contract for real rather than trusting the comment.
        """
        fernet_key = Fernet.generate_key()
        mock_fernet = self.safe_patch(
            "odoo.addons.cloudflare.models.website.WebsiteCloudflare._get_fernet"
        )
        mock_fernet.return_value = Fernet(fernet_key)

        website = self.env["website"].create(
            {
                "name": "Tunnel Provisioning Test Website",
                "domain": "https://tunnel-provisioning-test.example.com",
            }
        )
        website.write(
            {
                "cloudflare_api_token": "tok",
                "cloudflare_zone_id": "zone",
                "cloudflare_account_id": "acct",
            }
        )

        tunnel = self.env["cloudflare.tunnel"].create(
            {
                "cf_tunnel_id": "cftun_provisioning_test",
                "name": "Provisioning Test Tunnel",
                "website_id": website.id,
            }
        )
        # action_ensure_tunnel_running() is an @api.model method that
        # always operates on self.env["cloudflare.tunnel"].search([],
        # limit=1) regardless of what recordset it's called through --
        # confirm that search actually resolves to the tunnel this test
        # just created before relying on it, rather than silently testing
        # whatever unrelated tunnel happened to be first.
        self.assertEqual(
            self.env["cloudflare.tunnel"].search([], limit=1),
            tunnel,
            "test setup assumption: this must be the only/first tunnel "
            "visible to action_ensure_tunnel_running()'s own search()",
        )

        utils = self.env["zero_sudo.security.utils"]
        key = "cloudflare.tunnel.provisioned"
        svc_uid = utils._get_service_uid("cloudflare.user_cloudflare_tunnel")
        utils.with_user(svc_uid)._set_system_param(key, False)
        self.env.registry.clear_cache()

        self.safe_patch(
            "odoo.addons.cloudflare.models.tunnel.get_cfd_tunnel_token",
            return_value=(True, "faketoken"),
        )
        self.safe_patch(
            "odoo.addons.cloudflare.models.tunnel.CloudflareTunnel.action_push_configuration",
            side_effect=RuntimeError("simulated Cloudflare API failure"),
        )
        mock_start_daemon = self.safe_patch(
            "odoo.addons.cloudflare.models.tunnel.start_tunnel_daemon"
        )

        self.env["cloudflare.tunnel"].action_ensure_tunnel_running()

        mock_start_daemon.assert_called_once_with("faketoken")
        self.assertFalse(
            utils.with_user(svc_uid)._get_system_param(key),
            "A failed push must not mark the tunnel as provisioned, or it "
            "would never retry.",
        )

    def test_push_configuration_merges_global_and_tunnel_routes(self):
        # Tests [@ANCHOR: cloudflare:COMM_tunnel_action_push_configuration]

        # Tests [@ANCHOR: COMM_cf_tunnel_views_render]
        """
        action_push_configuration() must merge this tunnel's own routes
        with global route templates (tunnel_id=False), sorted by
        sequence, then append the SSH route (derived from the website's
        domain) and the mandatory catch-all -- and send the whole ingress
        list to Cloudflare in one call. Also the render-proof this
        session's tunnel_views.xml audit-ignore-view comment cites for
        view_cloudflare_tunnel_form's "Push Routing Config" button, which
        drives exactly this method.
        """
        fernet_key = Fernet.generate_key()
        mock_fernet = self.safe_patch(
            "odoo.addons.cloudflare.models.website.WebsiteCloudflare._get_fernet"
        )
        mock_fernet.return_value = Fernet(fernet_key)

        website = self.env["website"].create(
            {
                "name": "Push Config Test Website",
                "domain": "https://push-config-test.example.com",
            }
        )
        website.write(
            {
                "cloudflare_api_token": "tok",
                "cloudflare_zone_id": "zone",
                "cloudflare_account_id": "acct",
            }
        )

        tunnel = self.env["cloudflare.tunnel"].create(
            {
                "cf_tunnel_id": "cftun_push_test",
                "name": "Push Config Test Tunnel",
                "website_id": website.id,
            }
        )
        self.env["cloudflare.tunnel.route"].create(
            {
                "tunnel_id": tunnel.id,
                "hostname": "api.example.com",
                "service_url": "http://internal-api:8080",
                "sequence": 10,
            }
        )
        self.env["cloudflare.tunnel.route"].create(
            {
                "tunnel_id": False,
                "hostname": "global.example.com",
                "service_url": "http://internal-global:9090",
                "sequence": 5,
            }
        )

        mock_push = self.safe_patch(
            "odoo.addons.cloudflare.models.tunnel.update_cfd_tunnel_configuration",
            return_value=(True, "ok"),
        )

        tunnel.action_push_configuration()

        mock_push.assert_called_once()
        _account_id, _token, _cf_tunnel_id, payload = mock_push.call_args[0]
        ingress = payload["config"]["ingress"]
        # Bug fix (night-watch, 2026-09-17): this used to assertEqual the
        # *entire* hostname list, which silently assumed this test's two
        # routes were the only tunnel_id=False global routes in the whole
        # database. That's false as soon as ham_base is also installed --
        # ham_base/data/cloudflare_routes.xml ships four real, intentional,
        # noupdate=1 global template routes of its own (cf_route_relay_
        # bridge/simulated_band/adif_processor/gdpr_export, sequence
        # 20-50, no hostname), for other real hams.com backend services.
        # Those aren't test pollution or a leak to isolate -- they're
        # legitimate production config that's just as present in any real
        # deployment as in a combined `-u ham_base,...,cloudflare` test
        # run, which is exactly when this test used to fail (see
        # cloudflare-three-preexisting-test-failures-035f2dfc.md item 3).
        # Assert the real behavior under test -- merging, sequence-sort
        # ordering, and the mandatory trailing SSH/catch-all -- without
        # assuming this test owns every global route in the database.
        hostnames = [rule.get("hostname") for rule in ingress]
        self.assertIn("global.example.com", hostnames)
        self.assertIn("api.example.com", hostnames)
        self.assertLess(
            hostnames.index("global.example.com"),
            hostnames.index("api.example.com"),
            "Global route (sequence 5) must sort before the tunnel's own "
            "route (sequence 10).",
        )
        self.assertEqual(
            hostnames[-2:],
            ["ssh.push-config-test.example.com", None],
            "The SSH route and the hostname-less catch-all must always be "
            "the last two ingress entries, appended after every routed "
            "sequence.",
        )
        # burn-ignore-cloudflared-ingress: asserting on the same real,
        # architecturally correct localhost targets tunnel.py's own
        # ingress config uses (cloudflared runs on the same host as the
        # services it proxies to).
        self.assertEqual(ingress[-2]["service"], "ssh://localhost:22")  # burn-ignore-cloudflared-ingress
        self.assertEqual(ingress[-1]["service"], "http://localhost:8069")  # burn-ignore-cloudflared-ingress
