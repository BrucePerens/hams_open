# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
from unittest.mock import MagicMock

import requests

from odoo.tests.common import tagged
from odoo.tools import convert_file
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestTrustedIpRanges(HamsTransactionCase):
    def _icp(self):
        return self.env["ir.config_parameter"]

    def _utils(self):
        return self.env["cloudflare.trusted_ip_utils"]

    def _enable_non_tunnel_mode(self):
        self._icp().set_param("cloudflare.trust_non_tunnel_peers", "True")

    # [@ANCHOR: test_trusted_ip_ranges]
    # Tests [@ANCHOR: cloudflare:get_effective_trusted_ip_ranges]
    # Bug-hunt fix (night_shift_todo/low/cloudflare-trusted-ip-allow-list-defaults-to-cloudflare-
    # ranges-on-tunnel-only-deployments-7d2b8f14.md): "Done when" requires a test that the default
    # list on a Tunnel deployment contains no Cloudflare range.
    def test_00_effective_ranges_are_empty_by_default_tunnel_only_deployment(self):
        self._icp().set_param("cloudflare.trusted_ip_ranges_auto", "")
        self._icp().set_param("cloudflare.trusted_ip_ranges_custom", "")
        ranges = self._utils()._get_effective_trusted_ip_ranges()
        self.assertEqual(ranges, [], "A Tunnel-only deployment must trust no Cloudflare range by default.")
        self.assertNotIn("173.245.48.0/20", ranges)
        self.assertNotIn("2400:cb00::/32", ranges)

    def test_00b_effective_ranges_stay_empty_even_with_a_custom_range_configured_if_mode_is_off(self):
        """Adding a custom range alone must not enable trust -- the single, explicit
        non-Tunnel-mode toggle gates the whole feature, auto AND custom."""
        self._icp().set_param("cloudflare.trusted_ip_ranges_custom", "203.0.113.0/24")
        ranges = self._utils()._get_effective_trusted_ip_ranges()
        self.assertEqual(ranges, [])

    def test_01_effective_ranges_default_to_the_baked_in_snapshot_once_mode_is_on(self):
        self._enable_non_tunnel_mode()
        self._icp().set_param("cloudflare.trusted_ip_ranges_auto", "")
        self._icp().set_param("cloudflare.trusted_ip_ranges_custom", "")
        ranges = self._utils()._get_effective_trusted_ip_ranges()
        self.assertIn("173.245.48.0/20", ranges)
        self.assertIn("2400:cb00::/32", ranges)

    def test_02_effective_ranges_include_admin_custom_additions(self):
        self._enable_non_tunnel_mode()
        self._icp().set_param("cloudflare.trusted_ip_ranges_auto", "")
        self._icp().set_param("cloudflare.trusted_ip_ranges_custom", "203.0.113.0/24")
        ranges = self._utils()._get_effective_trusted_ip_ranges()
        self.assertIn("203.0.113.0/24", ranges)
        self.assertIn("173.245.48.0/20", ranges, "Custom additions must not replace the default.")

    def test_03_malformed_custom_range_is_skipped_not_fatal(self):
        self._enable_non_tunnel_mode()
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

    def test_04b_is_trusted_cf_peer_false_for_a_cloudflare_range_ip_when_mode_is_off(self):
        """The core of this bug-hunt fix: a real Cloudflare-published address is NOT a trusted
        peer on a Tunnel-only deployment (the default) even though it is a genuine Cloudflare
        edge IP -- this deployment's origin can never actually be reached from it."""
        self._icp().set_param("cloudflare.trusted_ip_ranges_auto", "")
        self.assertFalse(self._utils()._is_trusted_cf_peer("173.245.48.1"))

    def test_05_is_trusted_cf_peer_true_for_a_default_cloudflare_range_ip_once_mode_is_on(self):
        self._enable_non_tunnel_mode()
        self._icp().set_param("cloudflare.trusted_ip_ranges_auto", "")
        self.assertTrue(self._utils()._is_trusted_cf_peer("173.245.48.1"))

    def test_06_is_trusted_cf_peer_false_for_an_untrusted_ip(self):
        self._enable_non_tunnel_mode()
        self._icp().set_param("cloudflare.trusted_ip_ranges_auto", "")
        self._icp().set_param("cloudflare.trusted_ip_ranges_custom", "")
        self.assertFalse(self._utils()._is_trusted_cf_peer("8.8.8.8"))

    def test_07_is_trusted_cf_peer_true_once_admin_adds_a_custom_range(self):
        self._enable_non_tunnel_mode()
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

    # Tests [@ANCHOR: cloudflare:publish_trusted_ip_ranges_to_redis]
    def test_11_publish_with_non_tunnel_mode_off_writes_an_empty_list_that_outlives_the_daily_cron(self):
        """A Tunnel-only deployment publishes an empty list, not Cloudflare's ranges -- that is what
        replaces a wider list an older version of this module left in Redis. The key must also
        outlive the daily refresh interval, since the WSGI hook keeps its last-read value across a
        missing key and a freshly restarted worker has read nothing yet."""
        fake_redis = MagicMock()
        self.safe_patch(
            "odoo.addons.cloudflare.models.trusted_ip_ranges.get_redis_connection",
            return_value=fake_redis,
        )
        self._icp().set_param("cloudflare.trusted_ip_ranges_custom", "203.0.113.0/24")
        self._utils()._publish_trusted_ip_ranges_to_redis()
        fake_redis.set.assert_called_once()
        args, kwargs = fake_redis.set.call_args
        self.assertEqual(args, ("cloudflare:trusted_ip_ranges", "[]"))
        self.assertGreater(kwargs["ex"], 86400)

    # Tests [@ANCHOR: cloudflare:COMM_republish_trusted_ip_ranges_on_module_load]
    def test_12_module_upgrade_republishes_the_current_list_to_redis(self):
        """Loads the real data file the way `odoo -u cloudflare` does (mode='update'), so an upgrade
        of an existing install overwrites whatever Redis held before -- post_init_hook alone would
        only cover a fresh install."""
        fake_redis = MagicMock()
        self.safe_patch(
            "odoo.addons.cloudflare.models.trusted_ip_ranges.get_redis_connection",
            return_value=fake_redis,
        )
        convert_file(
            self.env, "cloudflare", "data/republish_trusted_ip_ranges.xml", {}, mode="update"
        )
        fake_redis.set.assert_called_once()
        self.assertEqual(fake_redis.set.call_args.args, ("cloudflare:trusted_ip_ranges", "[]"))

        self._enable_non_tunnel_mode()
        self._icp().set_param("cloudflare.trusted_ip_ranges_auto", "")
        fake_redis.reset_mock()
        convert_file(
            self.env, "cloudflare", "data/republish_trusted_ip_ranges.xml", {}, mode="update"
        )
        self.assertIn("173.245.48.0/20", fake_redis.set.call_args.args[1])
