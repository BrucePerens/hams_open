# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
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
