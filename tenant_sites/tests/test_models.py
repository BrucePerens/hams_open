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
