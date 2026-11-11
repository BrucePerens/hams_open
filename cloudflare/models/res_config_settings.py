# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
import time
from odoo import api, models, fields, _
from odoo.exceptions import UserError
import logging
from ..utils.cloudflare_api import create_cfd_tunnel, get_cfd_tunnel_token

_logger = logging.getLogger(__name__)


# # Verified by [@ANCHOR: test_xpath_rendering_cf_settings]
SECRET_MASK = "********"
SECRET_FIELDS = ("cloudflare_api_token", "cloudflare_turnstile_secret")


class ResConfigSettings(models.TransientModel):
    _inherit = "res.config.settings"

    # The two stored credentials read back as a mask (compute) and are written only when the field was changed
    # (inverse): a settings read over RPC never returns the secret. [@ANCHOR: cloudflare:COMM_settings_masks_secrets]
    cloudflare_api_token = fields.Char(
        compute="_compute_cloudflare_secrets", inverse="_inverse_cloudflare_secrets"
    )
    cloudflare_zone_id = fields.Char(
        related="website_id.cloudflare_zone_id", readonly=False
    )
    cloudflare_account_id = fields.Char(
        related="website_id.cloudflare_account_id", readonly=False
    )
    cloudflare_turnstile_secret = fields.Char(
        compute="_compute_cloudflare_secrets", inverse="_inverse_cloudflare_secrets"
    )

    # Off by default: a Tunnel-only deployment's origin is never reachable from Cloudflare's
    # published ranges at all (the only peer that can ever connect is loopback, trusted
    # unconditionally elsewhere), so there is no reason to trust those ranges too. See
    # trusted_ip_ranges.py's own docstring for why this single toggle gates the whole feature
    # (auto-fetched snapshot AND the admin's own custom additions below) and what else turning it
    # on actually requires to mean anything.
    cloudflare_trust_non_tunnel_peers = fields.Boolean(
        string="Trust Cloudflare's Published IP Ranges",
        config_parameter="cloudflare.trust_non_tunnel_peers",
        help=(
            "Only turn this on if Cloudflare connects to this origin directly over "
            "the network instead of through a Cloudflare Tunnel. Leave it off for a "
            "Tunnel-only deployment: cloudflared's connection is loopback and is "
            "already trusted unconditionally. Turning this on also needs "
            "authenticated origin pulls (Cloudflare's mTLS feature) or a per-zone "
            "firewall rule restricting the origin to Cloudflare's edge -- set up "
            "separately -- or it only adds trust without adding real protection, "
            "since Cloudflare's ranges front every one of its customers, not just "
            "this zone."
        ),
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

    # [@ANCHOR: cloudflare:COMM_settings_compute_trusted_ip_display]
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

    # [@ANCHOR: cloudflare:COMM_settings_compute_secrets]
    @api.depends("website_id")
    def _compute_cloudflare_secrets(self):
        for rec in self:
            for name in SECRET_FIELDS:
                rec[name] = SECRET_MASK if rec.website_id[name] else False

    # [@ANCHOR: cloudflare:COMM_settings_inverse_secrets]
    def _inverse_cloudflare_secrets(self):
        # The mask means "not edited"; a blank value clears the secret.
        for rec in self:
            vals = {n: rec[n] or False for n in SECRET_FIELDS if rec[n] != SECRET_MASK}
            if vals and rec.website_id:
                rec.website_id.write(vals)

    # [@ANCHOR: cloudflare:COMM_settings_get_values]
    @api.private
    @api.model
    def get_values(self):
        res = super().get_values()
        website = self.env["website"].get_current_website()
        for name in SECRET_FIELDS:
            if website[name]:
                res[name] = SECRET_MASK
        return res

    # [@ANCHOR: cloudflare:COMM_settings_set_values]
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
        # `command` embeds a real,
        # one-time Cloudflare tunnel install token. It is deliberately
        # NOT passed to create() -- cloudflare.tunnel.wizard.command is
        # a non-stored field computed from this action's own context,
        # so the token is never written to a DB column at all. It
        # lives only in this one response, for this one display.
        wizard = self.env["cloudflare.tunnel.wizard"].create({})

        return {
            "name": _("Cloudflare Tunnel Command"),
            "type": "ir.actions.act_window",
            "res_model": "cloudflare.tunnel.wizard",
            "res_id": wizard.id,
            "view_mode": "form",
            "target": "new",
            "context": {"install_command": command},
        }
