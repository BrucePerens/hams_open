# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later

from odoo.exceptions import AccessError
from odoo.tests.common import tagged

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

FOREIGN_KEY = "zz_foreign.setting"


@tagged("post_install", "-at_install")
class TestConfigParameterAllowlist(HamsTransactionCase):
    """A service group sees and writes only its own ir.config_parameter keys, never another module's (web.base.url, stripe.*)."""

    def _check(self, group_xmlid, own_key, write_key):
        """own_key: a key the group may read; write_key: a key it may write, or None when it has no write access."""
        group = self.env.ref(group_xmlid)
        user = self.env["res.users"].with_context(active_test=False).search([("group_ids", "in", group.id)], limit=1)
        self.assertTrue(user, "no service account in %s" % group_xmlid)
        params = self.env["ir.config_parameter"]
        own_row = params.search([("key", "=", own_key)]) or params.create({"key": own_key, "value": "x"})
        foreign_row = params.create({"key": FOREIGN_KEY, "value": "x"})
        as_service = params.with_user(user)
        self.assertEqual(as_service.search([("key", "=", own_key)]).ids, own_row.ids)
        self.assertFalse(as_service.search([("key", "=", FOREIGN_KEY)]))
        with self.assertRaises(AccessError):
            foreign_row.with_user(user).read(["value"])
        with self.assertRaises(AccessError):
            as_service.create({"key": FOREIGN_KEY + ".new", "value": "x"})
        if write_key:
            created = as_service.search([("key", "=", write_key)]) or as_service.create({"key": write_key, "value": "x"})
            created.write({"value": "y"})
            self.assertEqual(created.value, "y")
            with self.assertRaises(AccessError):
                foreign_row.with_user(user).write({"value": "y"})

    def test_purge_reads_and_writes_only_its_own_keys(self):
        self._check('cloudflare.group_cloudflare_purge', 'cloudflare.probe', None)

    def test_trusted_ip_reads_and_writes_only_its_own_keys(self):
        self._check('cloudflare.group_cloudflare_trusted_ip', 'cloudflare.trust_non_tunnel_peers', 'cloudflare.trusted_ip_ranges_auto')
