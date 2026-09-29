# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
import time
from odoo import models, fields, _
from odoo.exceptions import UserError
from ..utils.cloudflare_api import create_cfd_tunnel, get_cfd_tunnel_token


# # Verified by [@ANCHOR: test_xpath_rendering_cf_settings]
class ResConfigSettings(models.TransientModel):
    _inherit = "res.config.settings"

    cloudflare_api_token = fields.Char(
        related="website_id.cloudflare_api_token", readonly=False
    )
    cloudflare_zone_id = fields.Char(
        related="website_id.cloudflare_zone_id", readonly=False
    )
    cloudflare_account_id = fields.Char(
        related="website_id.cloudflare_account_id", readonly=False
    )
    cloudflare_turnstile_secret = fields.Char(
        related="website_id.cloudflare_turnstile_secret", readonly=False
    )

    # [@ANCHOR: cloudflare:COMM_trusted_ip_ranges_settings_fields]
    # Admin-supplied additions on top of the auto-fetched list below; never touched by the daily
    # refresh cron (see trusted_ip_ranges.py's own docstring for why they're kept separate).
    # `config_parameter=` is Odoo's own built-in read/write-through to ir.config_parameter, so no
    # manual get_values()/set_values() plumbing is needed for this field specifically.
    # Char, not Text: res.config.settings._get_classified_fields() only accepts
    # boolean/integer/float/char/selection/many2one/datetime for a settings-screen field --
    # Text raises there. Char has no fixed size in Odoo/Postgres unless size= is given, so it
    # still holds the full multi-line CIDR list without truncation.
    cloudflare_trusted_ip_ranges_custom = fields.Char(
        string="Additional Trusted IP Ranges",
        config_parameter="cloudflare.trusted_ip_ranges_custom",
    )
    cloudflare_trusted_ip_ranges_auto = fields.Char(
        string="Auto-Fetched Cloudflare IP Ranges",
        compute="_compute_cloudflare_trusted_ip_ranges_display",
    )
    cloudflare_trusted_ip_ranges_last_refreshed = fields.Char(
        string="Last Refreshed",
        compute="_compute_cloudflare_trusted_ip_ranges_display",
    )

    def _compute_cloudflare_trusted_ip_ranges_display(self):
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_trusted_ip"
        )
        icp = self.env["ir.config_parameter"].with_user(svc_uid)
        for record in self:
            record.cloudflare_trusted_ip_ranges_auto = (
                icp.get_param("cloudflare.trusted_ip_ranges_auto")
                or _("(not yet refreshed -- using the built-in default snapshot)")
            )
            record.cloudflare_trusted_ip_ranges_last_refreshed = icp.get_param(
                "cloudflare.trusted_ip_ranges_last_refreshed"
            ) or _("Never")

    def set_values(self):
        super().set_values()
        # Publish the custom-ranges edit to Redis immediately, so it takes effect for the
        # env-less WSGI hook (wsgi_proxy_scheme.py) right away instead of waiting for the
        # next daily cron tick or that hook's own up-to-60s cache TTL to naturally expire.
        self.env["cloudflare.trusted_ip_utils"]._publish_trusted_ip_ranges_to_redis()

    # [@ANCHOR: cloudflare:COMM_action_refresh_cloudflare_trusted_ip_ranges]
    def action_refresh_cloudflare_trusted_ip_ranges(self):
        self.env["cloudflare.trusted_ip_utils"]._cron_refresh_cloudflare_ip_ranges()
        return {
            "type": "ir.actions.client",
            "tag": "reload",
        }

    # [@ANCHOR: cloudflare:COMM_action_deploy_cf_waf]
    def action_deploy_cf_waf(self):
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_waf"
        )
        website_id = (
            self.website_id.id
            if self.website_id
            else self.env["website"].get_current_website().id
        )
        success, msg = (
            self.env["cloudflare.config.manager"]
            .with_user(svc_uid)
            .action_push_waf_rules(website_id=website_id)
        )
        if success:
            return {
                "type": "ir.actions.client",
                "tag": "display_notification",
                "params": {
                    "title": _("Success"),
                    "message": msg,
                    "type": "success",
                    "sticky": False,
                },
            }
        else:
            raise UserError(_("Failed to deploy WAF rules: %s") % msg)

    # [@ANCHOR: cloudflare:COMM_action_pull_cf_waf]
    def action_pull_cf_waf(self):
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_waf"
        )
        website_id = (
            self.website_id.id
            if self.website_id
            else self.env["website"].get_current_website().id
        )
        success, msg = (
            self.env["cloudflare.config.manager"]
            .with_user(svc_uid)
            .action_pull_waf_rules(website_id=website_id)
        )
        if success:
            return {
                "type": "ir.actions.client",
                "tag": "display_notification",
                "params": {
                    "title": _("Success"),
                    "message": msg,
                    "type": "success",
                    "sticky": False,
                },
            }
        else:
            raise UserError(_("Failed to pull WAF rules: %s") % msg)

    def action_generate_tunnel_command(self):
        # [@ANCHOR: COMM_cf_tunnel_setup]

        # # Verified by [@ANCHOR: COMM_test_cf_tunnel_setup]
        self.ensure_one()
        website = (
            self.website_id
            if self.website_id
            else self.env["website"].get_current_website()
        )

        token = website.cloudflare_api_token
        account_id = website.cloudflare_account_id

        if not token or not account_id:
            raise UserError(
                _(
                    "You must provide both the Cloudflare API Token and Account ID to create a tunnel."
                )
            )

        tunnel_name = f"odoo-edge-tunnel-{int(time.time())}"

        success, result = create_cfd_tunnel(account_id, token, tunnel_name)
        if not success:
            raise UserError(_("Failed to create tunnel: %s") % result)

        tunnel_id = result
        success_token, token_val = get_cfd_tunnel_token(account_id, token, tunnel_id)
        if not success_token:
            raise UserError(_("Failed to retrieve tunnel token: %s") % token_val)

        command = f"cloudflared service install {token_val}"

        # ADR-0001: Headless Mutation Context
        wizard = self.env["cloudflare.tunnel.wizard"].create({"command": command})

        return {
            "name": _("Cloudflare Tunnel Command"),
            "type": "ir.actions.act_window",
            "res_model": "cloudflare.tunnel.wizard",
            "res_id": wizard.id,
            "view_mode": "form",
            "target": "new",
        }
