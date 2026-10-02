# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
from unittest.mock import MagicMock

import requests

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestTrustedIpRanges(HamsTransactionCase):
    def _icp(self):
        return self.env["ir.config_parameter"]

    def _utils(self):
        return self.env["cloudflare.trusted_ip_utils"]

    # [@ANCHOR: test_trusted_ip_ranges]
    # The module docstring's own `Verified by [@ANCHOR: test_trusted_ip_ranges]`
    # already names this test for the whole merged-ranges feature, not just
    # the one helper it calls directly.
    # Tests [@ANCHOR: cloudflare:trusted_ip_ranges]
    # Tests [@ANCHOR: cloudflare:get_effective_trusted_ip_ranges]
    def test_01_effective_ranges_default_to_the_baked_in_snapshot(self):
        self._icp().set_param("cloudflare.trusted_ip_ranges_auto", "")
        self._icp().set_param("cloudflare.trusted_ip_ranges_custom", "")
        ranges = self._utils()._get_effective_trusted_ip_ranges()
        self.assertIn("173.245.48.0/20", ranges)
        self.assertIn("2400:cb00::/32", ranges)

    def test_02_effective_ranges_include_admin_custom_additions(self):
        self._icp().set_param("cloudflare.trusted_ip_ranges_auto", "")
        self._icp().set_param("cloudflare.trusted_ip_ranges_custom", "203.0.113.0/24")
        ranges = self._utils()._get_effective_trusted_ip_ranges()
        self.assertIn("203.0.113.0/24", ranges)
        self.assertIn("173.245.48.0/20", ranges, "Custom additions must not replace the default.")

    def test_03_malformed_custom_range_is_skipped_not_fatal(self):
        self._icp().set_param("cloudflare.trusted_ip_ranges_auto", "")
        self._icp().set_param(
            "cloudflare.trusted_ip_ranges_custom", "not-a-cidr\n203.0.113.0/24\n"
        )
        ranges = self._utils()._get_effective_trusted_ip_ranges()
        self.assertIn("203.0.113.0/24", ranges)
        self.assertNotIn("not-a-cidr", ranges)

    # Tests [@ANCHOR: cloudflare:is_trusted_cf_peer_with_env]
    def test_04_is_trusted_cf_peer_true_for_loopback(self):
        self.assertTrue(self._utils()._is_trusted_cf_peer("127.0.0.1"))  # burn-ignore-ssrf-test-value
        self.assertTrue(self._utils()._is_trusted_cf_peer("::1"))

    def test_05_is_trusted_cf_peer_true_for_a_default_cloudflare_range_ip(self):
        self._icp().set_param("cloudflare.trusted_ip_ranges_auto", "")
        self.assertTrue(self._utils()._is_trusted_cf_peer("173.245.48.1"))

    def test_06_is_trusted_cf_peer_false_for_an_untrusted_ip(self):
        self._icp().set_param("cloudflare.trusted_ip_ranges_auto", "")
        self._icp().set_param("cloudflare.trusted_ip_ranges_custom", "")
        self.assertFalse(self._utils()._is_trusted_cf_peer("8.8.8.8"))

    def test_07_is_trusted_cf_peer_true_once_admin_adds_a_custom_range(self):
        self._icp().set_param("cloudflare.trusted_ip_ranges_auto", "")
        self._icp().set_param("cloudflare.trusted_ip_ranges_custom", "203.0.113.0/24")
        self.assertTrue(self._utils()._is_trusted_cf_peer("203.0.113.55"))

    def test_08_is_trusted_cf_peer_false_for_unparseable_remote_addr(self):
        self.assertFalse(self._utils()._is_trusted_cf_peer("not-an-ip"))
        self.assertFalse(self._utils()._is_trusted_cf_peer(None))

    # Tests [@ANCHOR: cloudflare:cron_refresh_cloudflare_ip_ranges]
    def test_09_cron_refresh_updates_the_auto_list_on_success(self):
        fake_v4 = MagicMock(text="203.0.113.0/24\n", raise_for_status=lambda: None)
        fake_v6 = MagicMock(text="2001:db8::/32\n", raise_for_status=lambda: None)
        self.safe_patch(
            "odoo.addons.cloudflare.models.trusted_ip_ranges.requests.get",
            side_effect=[fake_v4, fake_v6],
        )
        self.safe_patch(
            "odoo.addons.cloudflare.models.trusted_ip_ranges.get_redis_connection",
            return_value=MagicMock(),
        )
        self._utils()._cron_refresh_cloudflare_ip_ranges()
        auto_text = self._icp().get_param("cloudflare.trusted_ip_ranges_auto")
        self.assertIn("203.0.113.0/24", auto_text)
        self.assertIn("2001:db8::/32", auto_text)
        self.assertTrue(self._icp().get_param("cloudflare.trusted_ip_ranges_last_refreshed"))

    def test_10_cron_refresh_failure_never_overwrites_the_existing_list(self):
        self._icp().set_param("cloudflare.trusted_ip_ranges_auto", "203.0.113.0/24")
        self.safe_patch(
            "odoo.addons.cloudflare.models.trusted_ip_ranges.requests.get",
            side_effect=requests.exceptions.ConnectionError("no route to host"),
        )
        self._utils()._cron_refresh_cloudflare_ip_ranges()
        self.assertEqual(
            self._icp().get_param("cloudflare.trusted_ip_ranges_auto"),
            "203.0.113.0/24",
            "A failed refresh must leave the last known-good list untouched.",
        )

    # Tests [@ANCHOR: cloudflare:COMM_trusted_ip_ranges_settings_fields]
    # Tests [@ANCHOR: cloudflare:publish_trusted_ip_ranges_to_redis]
    def test_11_settings_custom_field_is_config_parameter_backed_and_republishes(self):
        """The settings-screen `cloudflare_trusted_ip_ranges_custom` field is a plain
        `config_parameter=` passthrough (res_config_settings.py), so saving the settings
        form must both persist the value to ir.config_parameter and re-publish the merged
        list to Redis immediately (set_values()), rather than waiting for the next cron
        tick -- this is the real reason set_values() is overridden at all."""
        redis_mock = MagicMock()
        self.safe_patch(
            "odoo.addons.cloudflare.models.trusted_ip_ranges.get_redis_connection",
            return_value=redis_mock,
        )
        settings = self.env["res.config.settings"].create(
            {"cloudflare_trusted_ip_ranges_custom": "203.0.113.0/24"}
        )
        settings.set_values()
        self.assertEqual(
            self._icp().get_param("cloudflare.trusted_ip_ranges_custom"),
            "203.0.113.0/24",
            "The settings field must persist through Odoo's own config_parameter= plumbing.",
        )
        redis_mock.set.assert_called_once()
        published = redis_mock.set.call_args.args[1]
        self.assertIn("203.0.113.0/24", published)

    # Tests [@ANCHOR: cloudflare:COMM_action_refresh_cloudflare_trusted_ip_ranges]
    def test_12_refresh_action_invokes_the_cron_method_and_reloads(self):
        """The admin-facing "Refresh now" button on the settings page must run the exact
        same refresh logic as the daily cron (not a separate, divergent code path), then
        reload the settings page so the admin immediately sees the new auto-fetched list
        and last-refreshed timestamp."""
        mock_cron = self.safe_patch(
            "odoo.addons.cloudflare.models.trusted_ip_ranges."
            "CloudflareTrustedIpUtils._cron_refresh_cloudflare_ip_ranges"
        )
        settings = self.env["res.config.settings"].create({})
        result = settings.action_refresh_cloudflare_trusted_ip_ranges()
        self.assertTrue(mock_cron.called, "The button must call the same cron refresh method.")
        self.assertEqual(result, {"type": "ir.actions.client", "tag": "reload"})
