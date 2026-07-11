# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0.
import time
from odoo import models, fields, _
from odoo.exceptions import UserError
from ..utils.cloudflare_api import create_cfd_tunnel, get_cfd_tunnel_token


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
        # [@ANCHOR: cf_tunnel_setup]
        # Verified by [@ANCHOR: test_cf_tunnel_setup]
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
        wizard = (
            self.env["cloudflare.tunnel.wizard"]
            .with_context(mail_notrack=True)
            .create({"command": command})
        )

        return {
            "name": _("Cloudflare Tunnel Command"),
            "type": "ir.actions.act_window",
            "res_model": "cloudflare.tunnel.wizard",
            "res_id": wizard.id,
            "view_mode": "form",
            "target": "new",
        }
