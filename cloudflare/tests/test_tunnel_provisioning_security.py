# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
import importlib.util
import os

from cryptography.fernet import Fernet
from odoo.exceptions import UserError, ValidationError
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

MIGRATION_PATH = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    "migrations", "1.5", "post-ssh-route-off.py",
)


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
        # action_ensure_tunnel_running() is an @api.model method that now
        # iterates EVERY cloudflare.tunnel record (2026-09-19, Bruce's
        # multi-website answer) rather than operating on search([], limit=1).
        # This test only cares about the one tunnel it created, so assert it
        # is genuinely among the records that method will visit, and drive
        # the per-tunnel helper directly for the actual assertions below --
        # that keeps this test about the push-failure contract instead of
        # about whatever other tunnels a combined test run left lying around.
        self.assertIn(
            tunnel,
            self.env["cloudflare.tunnel"].search([], limit=10000),
            "test setup assumption: this tunnel must be visible to "
            "action_ensure_tunnel_running()'s own search()",
        )
        self.assertFalse(
            tunnel.routes_provisioned,
            "test setup assumption: a freshly created tunnel starts "
            "un-provisioned.",
        )

        self.safe_patch(
            "odoo.addons.cloudflare.models.tunnel.get_cfd_tunnel_token",
            return_value=(True, "faketoken"),
        )
        self.safe_patch(
            "odoo.addons.cloudflare.models.tunnel.CloudflareTunnel.action_push_configuration",
            side_effect=RuntimeError("simulated Cloudflare API failure"),
        )
        self.safe_patch(
            "odoo.addons.cloudflare.models.tunnel.is_tunnel_daemon_running",
            return_value=False,
        )
        mock_start_daemon = self.safe_patch(
            "odoo.addons.cloudflare.models.tunnel.start_tunnel_daemon"
        )

        outcome = self.env["cloudflare.tunnel"]._ensure_one_tunnel_running(tunnel)

        self.assertEqual(outcome, "started")
        mock_start_daemon.assert_called_once_with(
            "faketoken", tunnel_key="cftun_provisioning_test"
        )
        self.assertFalse(
            tunnel.routes_provisioned,
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
                # SSH is off by default; this test checks the ordering
                # of the trailing ssh rule, so opt in explicitly.
                "ssh_route_enabled": True,
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

    def _make_push_test_tunnel(self, label, domain):
        """A website with Cloudflare credentials and one tunnel on it,
        set up the same way the push test above does."""
        fernet_key = Fernet.generate_key()
        mock_fernet = self.safe_patch(
            "odoo.addons.cloudflare.models.website.WebsiteCloudflare._get_fernet"
        )
        mock_fernet.return_value = Fernet(fernet_key)
        website = self.env["website"].create(
            {"name": f"{label} Website", "domain": domain}
        )
        website.write(
            {
                "cloudflare_api_token": "tok",
                "cloudflare_zone_id": "zone",
                "cloudflare_account_id": "acct",
            }
        )
        return self.env["cloudflare.tunnel"].create(
            {
                "cf_tunnel_id": f"cftun_{label.lower().replace(' ', '_')}",
                "name": f"{label} Tunnel",
                "website_id": website.id,
            }
        )

    def test_push_configuration_orders_routes_and_refuses_shared_sequence(self):
        # Tests [@ANCHOR: cloudflare:COMM_tunnel_push_route_order]
        """
        cloudflared applies the first ingress rule that matches, so the
        pushed order must be explicit. Rules go out sorted by sequence
        (a tunnel route and a global route interleave by sequence, not
        by which recordset they came from), hostname and anchored path
        regexes pass through unchanged, and a sequence shared by two
        rules of the merged list makes the push fail before anything
        is sent to Cloudflare.
        """
        tunnel = self._make_push_test_tunnel(
            "Route Order", "https://route-order-test.example.com"
        )
        Route = self.env["cloudflare.tunnel.route"]
        Route.create(
            {
                "tunnel_id": tunnel.id,
                "hostname": "relay.route-order-test.example.com",
                "path": "^/ws/daemon_uplink$",
                "service_url": "ws://relay-bridge:8766",
                "sequence": 3,
            }
        )
        Route.create(
            {
                "tunnel_id": False,
                "path": "^/route-order-global$",
                "service_url": "http://global-service:9090",
                "sequence": 2,
            }
        )
        Route.create(
            {
                "tunnel_id": tunnel.id,
                "path": "^/route-order-first$",
                "service_url": "http://first-service:9091",
                "sequence": 1,
            }
        )
        mock_push = self.safe_patch(
            "odoo.addons.cloudflare.models.tunnel.update_cfd_tunnel_configuration",
            return_value=(True, "ok"),
        )

        tunnel.action_push_configuration()

        ingress = mock_push.call_args[0][3]["config"]["ingress"]
        own_paths = (
            "^/route-order-first$",
            "^/route-order-global$",
            "^/ws/daemon_uplink$",
        )
        # Select by hostname as well as path: another installed module
        # (ham_base) ships a global relay.hams.com rule with the same
        # ^/ws/daemon_uplink$ path, which a path-only filter would pick up.
        own_hostnames = (None, "relay.route-order-test.example.com")
        own_rules = [
            rule
            for rule in ingress
            if rule.get("path") in own_paths
            and rule.get("hostname") in own_hostnames
        ]
        self.assertEqual(
            own_rules,
            [
                {
                    "path": "^/route-order-first$",
                    "service": "http://first-service:9091",
                },
                {
                    "path": "^/route-order-global$",
                    "service": "http://global-service:9090",
                },
                {
                    "hostname": "relay.route-order-test.example.com",
                    "path": "^/ws/daemon_uplink$",
                    "service": "ws://relay-bridge:8766",
                },
            ],
        )

        # A global route that takes sequence 1 as well: the order of
        # the two is no longer defined, so nothing may be pushed.
        Route.create(
            {
                "tunnel_id": False,
                "path": "^/route-order-clash$",
                "service_url": "http://clash-service:9092",
                "sequence": 1,
            }
        )
        mock_push.reset_mock()
        with self.assertRaises(UserError) as caught:
            tunnel.action_push_configuration()
        self.assertIn("1", str(caught.exception))
        mock_push.assert_not_called()

    def test_push_configuration_ssh_route_follows_tunnel_switch(self):
        # Tests [@ANCHOR: cloudflare:COMM_tunnel_ssh_route_enabled]
        """
        The ssh.<domain> rule is an explicit per-tunnel switch that is
        OFF by default ("Don't expose ssh", Bruce, 2026-10-03). A push
        omits it until an administrator turns the switch on; turned
        back off it disappears again. A website with no domain gets no
        ssh rule either way.
        """
        tunnel = self._make_push_test_tunnel(
            "Ssh Switch", "https://ssh-switch-test.example.com"
        )
        self.assertFalse(tunnel.ssh_route_enabled)
        mock_push = self.safe_patch(
            "odoo.addons.cloudflare.models.tunnel.update_cfd_tunnel_configuration",
            return_value=(True, "ok"),
        )

        tunnel.action_push_configuration()
        ingress = mock_push.call_args[0][3]["config"]["ingress"]
        self.assertFalse(
            [
                rule for rule in ingress
                if rule["service"].startswith("ssh://")
                or rule.get("hostname", "").startswith("ssh.")
            ],
            "A default tunnel must not publish any ssh route.",
        )
        self.assertEqual(ingress[-1], {"service": "http://localhost:8069"})  # burn-ignore-cloudflared-ingress

        tunnel.ssh_route_enabled = True
        mock_push.reset_mock()
        tunnel.action_push_configuration()
        ingress = mock_push.call_args[0][3]["config"]["ingress"]
        self.assertEqual(
            ingress[-2]["hostname"], "ssh.ssh-switch-test.example.com"
        )

        tunnel.ssh_route_enabled = False
        mock_push.reset_mock()
        tunnel.action_push_configuration()
        ingress = mock_push.call_args[0][3]["config"]["ingress"]
        hostnames = [rule.get("hostname") for rule in ingress]
        self.assertNotIn("ssh.ssh-switch-test.example.com", hostnames)
        self.assertEqual(ingress[-1], {"service": "http://localhost:8069"})  # burn-ignore-cloudflared-ingress

        # Enabled but no website domain: still no ssh rule.
        no_domain_tunnel = self._make_push_test_tunnel("No Domain", False)
        no_domain_tunnel.ssh_route_enabled = True
        mock_push.reset_mock()
        no_domain_tunnel.action_push_configuration()
        ingress = mock_push.call_args[0][3]["config"]["ingress"]
        self.assertFalse(
            [rule for rule in ingress if "ssh://" in rule["service"]]
        )

    def test_ssh_route_default_false_and_migration_switches_existing_off(self):
        # Tests [@ANCHOR: cloudflare:COMM_tunnel_ssh_route_enabled]
        """
        New tunnels default to not publishing SSH, and the 1.5
        post-migration (the real file Odoo runs) turns it off on rows
        left True or NULL by 1.4, idempotently.
        """
        Tunnel = self.env["cloudflare.tunnel"]
        self.assertFalse(
            Tunnel.default_get(["ssh_route_enabled"]).get("ssh_route_enabled")
        )
        on = Tunnel.create({"cf_tunnel_id": "cftun_mig_on", "name": "Mig On"})
        null = Tunnel.create({"cf_tunnel_id": "cftun_mig_null", "name": "Mig Null"})
        off = Tunnel.create({"cf_tunnel_id": "cftun_mig_off", "name": "Mig Off"})
        on.ssh_route_enabled = True
        self.env.flush_all()
        self.env.cr.execute(  # audit-ignore-sql: test fixture, bound params
            "UPDATE cloudflare_tunnel SET ssh_route_enabled = NULL WHERE id = %s",
            (null.id,),
        )
        self.env.invalidate_all()

        spec = importlib.util.spec_from_file_location(
            "cloudflare_post_migration_1_5", MIGRATION_PATH
        )
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        for _ in range(2):  # second run proves idempotence
            module.migrate(self.env.cr, "1.4")
            self.env.invalidate_all()
            self.assertFalse((on | null | off).filtered("ssh_route_enabled"))
        self.assertFalse(Tunnel.search([("ssh_route_enabled", "=", True)]))

    def test_build_ingress_is_exactly_what_a_push_sends_and_needs_no_network(self):
        # Tests [@ANCHOR: cloudflare:COMM_tunnel_build_ingress]
        tunnel = self._make_push_test_tunnel("Build Ingress", "https://build-ingress-test.example.com")
        self.env["cloudflare.tunnel.route"].create(
            {
                "tunnel_id": tunnel.id,
                "hostname": "build-ingress-test.example.com",
                "path": "^/adif$",
                "service_url": "http://localhost:8070",  # burn-ignore-cloudflared-ingress
                "sequence": 1,
            }
        )
        mock_push = self.safe_patch(
            "odoo.addons.cloudflare.models.tunnel.update_cfd_tunnel_configuration",
            return_value=(True, "ok"),
        )
        planned = tunnel._build_ingress()
        mock_push.assert_not_called()
        self.assertEqual(planned[-1], {"service": tunnel.catch_all_service})
        self.assertIn(
            {
                "hostname": "build-ingress-test.example.com",
                "path": "^/adif$",
                "service": "http://localhost:8070",  # burn-ignore-cloudflared-ingress
            },
            planned,
        )
        tunnel.action_push_configuration()
        self.assertEqual(mock_push.call_args[0][3]["config"]["ingress"], planned)

    def test_a_push_is_refused_when_an_ingress_problem_is_reported(self):
        # Tests [@ANCHOR: cloudflare:COMM_tunnel_ingress_problems]
        tunnel = self._make_push_test_tunnel("Problems", "https://problems-test.example.com")
        mock_push = self.safe_patch(
            "odoo.addons.cloudflare.models.tunnel.update_cfd_tunnel_configuration",
            return_value=(True, "ok"),
        )
        self.assertEqual(tunnel._ingress_problems(tunnel._build_ingress()), [])
        mock_problems = self.safe_patch_object(
            type(tunnel), "_ingress_problems", return_value=["a rule exposes a tenant"]
        )
        with self.assertRaises(UserError) as caught:
            tunnel.action_push_configuration()
        self.assertIn("a rule exposes a tenant", str(caught.exception))
        mock_problems.assert_called()
        mock_push.assert_not_called()

    def test_push_configuration_catch_all_service_is_per_tunnel(self):
        # Tests [@ANCHOR: cloudflare:COMM_tunnel_catch_all_service]
        """
        The last ingress rule is the tunnel's own catch_all_service. A default tunnel keeps pushing this
        Odoo (the behaviour before the field existed); a tunnel whose unknown hostnames should be
        answered elsewhere points it at another service; a malformed value is refused when saved,
        never pushed.
        """
        tunnel = self._make_push_test_tunnel(
            "Catch All", "https://catch-all-test.example.com"
        )
        self.assertEqual(tunnel.catch_all_service, "http://localhost:8069")  # burn-ignore-cloudflared-ingress
        mock_push = self.safe_patch(
            "odoo.addons.cloudflare.models.tunnel.update_cfd_tunnel_configuration",
            return_value=(True, "ok"),
        )
        tunnel.action_push_configuration()
        ingress = mock_push.call_args[0][3]["config"]["ingress"]
        self.assertEqual(ingress[-1], {"service": "http://localhost:8069"})  # burn-ignore-cloudflared-ingress

        tunnel.catch_all_service = "http://localhost:18110"  # burn-ignore-cloudflared-ingress
        mock_push.reset_mock()
        tunnel.action_push_configuration()
        ingress = mock_push.call_args[0][3]["config"]["ingress"]
        self.assertEqual(ingress[-1], {"service": "http://localhost:18110"})  # burn-ignore-cloudflared-ingress
        self.assertEqual(
            [rule for rule in ingress if "hostname" not in rule and "path" not in rule][-1],
            ingress[-1],
        )

        tunnel.catch_all_service = "http_status:404"
        mock_push.reset_mock()
        tunnel.action_push_configuration()
        self.assertEqual(
            mock_push.call_args[0][3]["config"]["ingress"][-1], {"service": "http_status:404"}
        )

        # Test warning notification and log when catch_all_service points directly to port 8069 vs 8085
        tunnel.catch_all_service = "http://localhost:8069"  # burn-ignore-cloudflared-ingress
        mock_push.reset_mock()
        with self.assertLogs("odoo.addons.cloudflare.models.tunnel", level="WARNING") as cm:
            res_warn = tunnel.action_push_configuration()
        self.assertEqual(res_warn["params"]["type"], "warning")
        self.assertIn("8069", res_warn["params"]["message"])
        self.assertTrue(any(":8069" in msg for msg in cm.output))

        tunnel.catch_all_service = "http://localhost:8085"  # burn-ignore-cloudflared-ingress
        mock_push.reset_mock()
        res_ok = tunnel.action_push_configuration()
        self.assertEqual(res_ok["params"]["type"], "success")

        for bad in (
            "", "http_status:40", "http://a b", "no-scheme:8069",
            "ssh://host:22", "http://host:8069\nx",
        ):
            with self.subTest(bad=bad):
                with self.assertRaises(ValidationError), self.env.cr.savepoint():
                    tunnel.catch_all_service = bad
                    tunnel.flush_recordset()
