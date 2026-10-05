# Copyright © HAMS project. AGPL-3.0-or-later.
from odoo.exceptions import UserError
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

WIZ = "odoo.addons.cloudflare.models.zone_settings_wizard"
CREDS = "odoo.addons.cloudflare.models.website.WebsiteCloudflare._get_cloudflare_credentials"


@tagged("post_install", "-at_install")
class TestZoneSettingsAlwaysUseHttps(HamsTransactionCase):
    """The Zone Settings wizard can set Cloudflare's "Always Use HTTPS" for the zone.

    The edge redirect matters for the DX firehose: its login is the member's Odoo session cookie, so a plain
    ws:// handshake must never be accepted by the edge. Tests [@ANCHOR: cloudflare:COMM_zone_settings_always_use_https]
    """

    def test_01_apply_sends_always_use_https(self):
        wiz = self.env["cloudflare.zone.settings.wizard"].create({"always_use_https": "on"})
        self.safe_patch(CREDS, return_value=("tok", "zone"))
        upd = self.safe_patch(WIZ + ".update_zone_setting", return_value=(True, "ok"))
        wiz.action_apply_settings()
        upd.assert_called_once_with("always_use_https", "on", "tok", "zone")

    def test_02_unset_leaves_the_zone_alone(self):
        wiz = self.env["cloudflare.zone.settings.wizard"].create({})
        self.safe_patch(CREDS, return_value=("tok", "zone"))
        upd = self.safe_patch(WIZ + ".update_zone_setting", return_value=(True, "ok"))
        wiz.action_apply_settings()
        upd.assert_not_called()

    def test_03_default_get_reads_current_value(self):
        self.safe_patch(CREDS, return_value=("tok", "zone"))
        self.safe_patch(
            WIZ + ".get_zone_settings",
            return_value=[{"id": "always_use_https", "value": "off"}],
        )
        res = self.env["cloudflare.zone.settings.wizard"].default_get(["always_use_https"])
        self.assertEqual(res["always_use_https"], "off")

    def test_04_api_failure_is_reported(self):
        wiz = self.env["cloudflare.zone.settings.wizard"].create({"always_use_https": "on"})
        self.safe_patch(CREDS, return_value=("tok", "zone"))
        self.safe_patch(WIZ + ".update_zone_setting", return_value=(False, "API Error"))
        with self.assertRaises(UserError):
            wiz.action_apply_settings()

    def test_05_field_is_on_the_form(self):
        arch = self.env["cloudflare.zone.settings.wizard"].get_view(view_type="form")["arch"]
        self.assertIn("always_use_https", arch)
