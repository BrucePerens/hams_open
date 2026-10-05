# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.exceptions import AccessError, ValidationError
from odoo.tests import tagged

from .. import request_state
from ..hooks import post_init_hook


@tagged("post_install", "-at_install", "tenant_sites")
class TestTenantSitesModels(HamsTransactionCase):
    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.website = cls.env["website"].create({"name": "Tenant A", "domain": "https://a.tenant.example"})
        cls.site = cls.env["tenant.site"].create(
            {
                "name": "Tenant A",
                "website_id": cls.website.id,
                "host_ids": [(0, 0, {"name": "A.Tenant.Example"}), (0, 0, {"name": "www.a.tenant.example"})],
            }
        )

    # [@ANCHOR: tenant_sites:COMM_test_host_normalize]
    def test_host_names_are_normalized_and_validated(self):
        # Tests [@ANCHOR: tenant_sites:COMM_host_normalize]
        # Tests [@ANCHOR: tenant_sites:COMM_host_create]
        # Tests [@ANCHOR: tenant_sites:COMM_host_write]
        self.assertEqual(sorted(self.site.host_ids.mapped("name")), ["a.tenant.example", "www.a.tenant.example"])
        with self.assertRaises(ValidationError):
            self.env["tenant.site.host"].create({"name": "10.0.0.1", "site_id": self.site.id})
            self.env.flush_all()
        with self.assertRaises(ValidationError):
            self.site.host_ids[0].write({"name": "not a host"})
            self.env.flush_all()

    def test_a_hostname_belongs_to_one_site_only(self):
        other = self.env["website"].create({"name": "Tenant B"})
        other_site = self.env["tenant.site"].create({"name": "B", "website_id": other.id})
        with self.assertRaises(Exception), self.cr.savepoint():
            self.env["tenant.site.host"].create({"name": "a.tenant.example", "site_id": other_site.id})
            self.env.flush_all()
        with self.assertRaises(Exception), self.cr.savepoint():
            self.env["tenant.site"].create({"name": "again", "website_id": other.id})
            self.env.flush_all()
            self.env["tenant.site"].create({"name": "again 2", "website_id": other.id})
            self.env.flush_all()

    # [@ANCHOR: tenant_sites:COMM_test_own_host_normalize]
    def test_own_host_patterns(self):
        # Tests [@ANCHOR: tenant_sites:COMM_own_host_normalize]
        Own = self.env["tenant.site.own.host"]
        Own.create([{"name": "Main.Example"}, {"name": "*.Main.Example"}])
        self.assertEqual(sorted(Own.search([], limit=100).mapped("name")), ["*.main.example", "main.example"])
        for bad in ("*", "*.", "x y"):
            with self.assertRaises(ValidationError):
                Own.create({"name": bad})
                self.env.flush_all()

    # [@ANCHOR: tenant_sites:COMM_test_host_not_own]
    def test_a_tenant_cannot_take_a_main_site_hostname(self):
        # Tests [@ANCHOR: tenant_sites:COMM_host_not_own]
        self.env["tenant.site.own.host"].create({"name": "*.main.example"})
        with self.assertRaises(ValidationError):
            self.env["tenant.site.host"].create({"name": "relay.main.example", "site_id": self.site.id})
            self.env.flush_all()

    # [@ANCHOR: tenant_sites:COMM_test_own_host_patterns]
    def test_own_patterns_follow_the_data(self):
        # Tests [@ANCHOR: tenant_sites:COMM_own_host_patterns]
        Host = self.env["tenant.site.host"]
        self.assertEqual(Host._own_host_patterns(), ())
        own = self.env["tenant.site.own.host"].create({"name": "hams.example"})
        self.assertEqual(Host._own_host_patterns(), ("hams.example",))
        own.unlink()
        self.assertEqual(Host._own_host_patterns(), ())

    # [@ANCHOR: tenant_sites:COMM_test_website_id_for_host]
    # [@ANCHOR: tenant_sites:COMM_test_current_website_id]
    def test_tenant_hostnames_select_the_tenant_website(self):
        # Tests [@ANCHOR: tenant_sites:COMM_website_id_for_host]
        # Tests [@ANCHOR: tenant_sites:COMM_current_website_id]
        Website = self.env["website"]
        Host = self.env["tenant.site.host"]
        self.assertEqual(Website._get_current_website_id("a.tenant.example"), self.website.id)
        self.assertEqual(Website._get_current_website_id("WWW.A.Tenant.Example:8069"), self.website.id)
        first = Website.search([], limit=1).id
        self.assertEqual(Website._get_current_website_id("unrelated.example"), first)
        self.assertFalse(Website._get_current_website_id("unrelated.example", fallback=False))
        self.assertEqual(Host._website_id_for_host("garbage"), 0)
        self.site.active = False
        self.assertEqual(Host._website_id_for_host("a.tenant.example"), 0)
        self.site.active = True
        self.assertEqual(Host._website_id_for_host("a.tenant.example"), self.website.id)
        self.site.host_ids[0].unlink()
        self.assertEqual(Host._website_id_for_host("a.tenant.example"), 0)

    # [@ANCHOR: tenant_sites:COMM_test_request_website_id]
    # [@ANCHOR: tenant_sites:COMM_test_request_state]
    def test_outside_a_request_there_is_no_state_and_no_tenant_website(self):
        # Tests [@ANCHOR: tenant_sites:COMM_request_website_id]
        # Tests [@ANCHOR: tenant_sites:COMM_request_state]
        self.assertIsNone(request_state.state())
        self.assertEqual(self.env["tenant.site.host"]._tenant_request_website_id(), 0)

    # [@ANCHOR: tenant_sites:COMM_test_tenant_hosts_exist]
    def test_tenant_hosts_exist(self):
        # Tests [@ANCHOR: tenant_sites:COMM_tenant_hosts_exist]
        Host = self.env["tenant.site.host"]
        self.assertTrue(Host._tenant_hosts_exist())
        self.site.active = False
        self.assertFalse(Host._tenant_hosts_exist())

    # [@ANCHOR: tenant_sites:COMM_test_service_account_acl]
    def test_the_public_user_cannot_read_any_tenant_table(self):
        # Tests [@ANCHOR: tenant_sites:COMM_service_account_acl]
        public = self.env.ref("base.public_user")
        for model in ("tenant.site", "tenant.site.host", "tenant.site.own.host"):
            with self.assertRaises(AccessError):
                self.env[model].with_user(public).check_access("read")
        service_uid = self.env["zero_sudo.security.utils"]._get_service_uid("tenant_sites.user_tenant_sites_service")
        service = self.env["tenant.site.host"].with_user(service_uid)
        self.assertTrue(service.search([], limit=1))
        with self.assertRaises(AccessError):
            service.create({"name": "x.tenant.example", "site_id": self.site.id})

    # [@ANCHOR: tenant_sites:COMM_test_post_init_hook]
    def test_post_init_hook_installs_the_documentation_and_is_repeatable(self):
        # Tests [@ANCHOR: tenant_sites:COMM_post_init_hook]
        post_init_hook(self.env)
        post_init_hook(self.env)

    # [@ANCHOR: tenant_sites:COMM_test_ingress_problems]
    def test_an_unscoped_path_rule_is_refused_while_tenants_exist(self):
        # Tests [@ANCHOR: tenant_sites:COMM_ingress_problems]
        odoo_service = "http://odoo-test-service:8069"
        bus_service = "http://odoo-test-service:8072"
        tunnel = self.env["cloudflare.tunnel"].new(
            {"name": "t", "cf_tunnel_id": "x", "catch_all_service": odoo_service}
        )
        unscoped = [{"path": "^/websocket$", "service": bus_service}, {"service": odoo_service}]
        problems = tunnel._ingress_problems(unscoped)
        self.assertEqual(len(problems), 1)
        self.assertIn("^/websocket$", problems[0])
        scoped = [
            {"hostname": "hams.example", "path": "^/websocket$", "service": bus_service},
            {"service": odoo_service},
        ]
        self.assertEqual(tunnel._ingress_problems(scoped), [])
        same_as_catch_all = [{"path": "^/x$", "service": odoo_service}, {"service": odoo_service}]
        self.assertEqual(tunnel._ingress_problems(same_as_catch_all), [])
        self.site.active = False
        self.assertEqual(tunnel._ingress_problems(unscoped), [])


    # [@ANCHOR: tenant_sites:COMM_test_static_rule_guard]
    def _static_ingress(self, hosts, service="http://localhost:18201", drop=()):
        rules = [
            {"hostname": host, "path": "^/static/", "service": service} for host in hosts if host not in drop
        ]
        return rules + [{"service": "http://odoo-test-service:8069"}]

    def test_a_push_that_drops_a_tenants_static_rule_is_refused(self):
        # Tests [@ANCHOR: tenant_sites:COMM_static_rule_problems]
        tunnel = self.env["cloudflare.tunnel"].new(
            {"name": "t", "cf_tunnel_id": "x", "catch_all_service": "http://odoo-test-service:8069"}
        )
        hosts = ["a.tenant.example", "www.a.tenant.example"]
        # No static service recorded: nothing to guard.
        self.assertEqual(tunnel._ingress_problems(self._static_ingress([])), [])
        self.site.static_service = "http://localhost:18201"
        self.assertEqual(tunnel._ingress_problems(self._static_ingress(hosts)), [])
        # One hostname's rule gone: refused, naming it (and only it).
        problems = tunnel._ingress_problems(self._static_ingress(hosts, drop=("www.a.tenant.example",)))
        self.assertEqual(len(problems), 1)
        self.assertIn("www.a.tenant.example", problems[0])
        self.assertNotIn("for a.tenant.example", problems[0])
        # All gone: both named.
        problems = tunnel._ingress_problems(self._static_ingress([]))
        self.assertEqual(len(problems), 1)
        self.assertIn("a.tenant.example, www.a.tenant.example", problems[0])
        # A rule to a different service, or with a different path, does not count.
        self.assertEqual(len(tunnel._ingress_problems(self._static_ingress(hosts, service="http://localhost:9"))), 1)
        wrong_path = [{"hostname": h, "path": "^/other/", "service": "http://localhost:18201"} for h in hosts]
        wrong_path.append({"service": "http://odoo-test-service:8069"})
        self.assertEqual(len(tunnel._ingress_problems(wrong_path)), 1)
        # A hostless rule does not stand in for the hostname-scoped one.
        hostless = [{"path": "^/static/", "service": "http://localhost:18201"}, {"service": "http://odoo-test-service:8069"}]
        self.assertGreaterEqual(len(tunnel._ingress_problems(hostless)), 1)
        # An inactive site, or a cleared service, is not guarded.
        self.site.static_service = False
        self.assertEqual(tunnel._ingress_problems(self._static_ingress([])), [])
        self.site.static_service = "http://localhost:18201"
        self.site.active = False
        self.assertEqual(tunnel._ingress_problems(self._static_ingress([])), [])

    def test_a_real_push_is_refused_before_anything_is_sent(self):
        """The guard sits behind action_push_configuration: no Cloudflare call is made when it refuses."""
        from unittest.mock import patch
        from odoo.exceptions import UserError

        tunnel = self.env["cloudflare.tunnel"].create(
            {"name": "guard tunnel", "cf_tunnel_id": "guard-x", "catch_all_service": "http://odoo-test-service:8069",
             "website_id": self.env["website"].search([], limit=1).id}
        )
        self.site.static_service = "http://localhost:18201"
        with patch.object(type(tunnel), "_build_ingress", return_value=self._static_ingress([])), \
             patch.object(type(self.env["website"]), "_get_cloudflare_credentials", return_value=("tok", "zone")), \
             patch.object(type(self.env["website"]), "cloudflare_account_id", "acct", create=True), \
             patch("odoo.addons.cloudflare.models.tunnel.update_cfd_tunnel_configuration") as push:
            with self.assertRaises(UserError) as caught:
                tunnel.action_push_configuration()
            self.assertIn("serves /static/", str(caught.exception))
            push.assert_not_called()

    def test_the_1_1_migration_adopts_existing_static_rules_only_when_every_host_has_one(self):
        import importlib.util
        import os

        path = os.path.join(os.path.dirname(os.path.dirname(__file__)), "migrations", "1.1", "post-adopt-static-services.py")
        spec = importlib.util.spec_from_file_location("tenant_sites_adopt_static", path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        website_b = self.env["website"].create({"name": "Tenant B static"})
        site_b = self.env["tenant.site"].create(
            {"name": "B", "website_id": website_b.id, "host_ids": [(0, 0, {"name": "b.tenant.example"})]}
        )
        website_c = self.env["website"].create({"name": "Tenant C static"})
        site_c = self.env["tenant.site"].create(
            {"name": "C", "website_id": website_c.id,
             "host_ids": [(0, 0, {"name": "c.tenant.example"}), (0, 0, {"name": "www.c.tenant.example"})]}
        )
        Route = self.env["cloudflare.tunnel.route"]
        seq = 91000
        for host in ("b.tenant.example", "c.tenant.example"):  # c has only one of its two hosts routed
            seq += 1
            Route.create({"sequence": seq, "hostname": host, "path": "^/static/", "service_url": "http://localhost:18201"})
        seq += 1
        Route.create({"sequence": seq, "hostname": "a.tenant.example", "path": "^/static/", "service_url": "http_status:404"})
        self.env.flush_all()
        module.migrate(self.env.cr, "1.0")
        self.env.invalidate_all()
        self.assertEqual(site_b.static_service, "http://localhost:18201")
        self.assertFalse(site_c.static_service)
        self.assertFalse(self.site.static_service)  # an http_status rule is not a service
        site_b.static_service = "http://localhost:7"
        self.env.flush_all()
        module.migrate(self.env.cr, "1.0")
        self.env.invalidate_all()
        self.assertEqual(site_b.static_service, "http://localhost:7")  # a value already set is left alone
