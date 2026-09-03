# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
from odoo import models, api, fields, _
from odoo.exceptions import AccessError


class CloudflareWAF(models.AbstractModel):
    _name = "cloudflare.waf"
    _description = "Cloudflare WAF Interface"
    name = fields.Char(string="Name", default=lambda self: self._description)

    @api.model
    def ban_ip(
        # [@ANCHOR: COMM_cf_ban_ip_api]
        self,
        ip_address,
        mode="block",
        duration=3600,
        notes="Honeypot Triggered",
        website_id=None,
    ):
        # Adversarial security review, 2026-09-03: this is a public
        # (non-underscore-prefixed) @api.model method, directly
        # dispatchable via /web/dataset/call_kw by any authenticated
        # session (auth="user" is all Odoo's own RPC dispatch requires --
        # this AbstractModel has no ir.model.access.csv row of its own,
        # since it's a service interface, not a stored model). It used to
        # escalate straight to the cloudflare.user_cloudflare_waf service
        # account with zero check on who the real caller was, so any
        # portal user could trigger a real Cloudflare Firewall Access
        # Rules API call against any website's real, live production zone
        # -- a real, unauthenticated-in-effect DoS primitive (ban a
        # legitimate admin's or monitoring system's own IP) and a real
        # Cloudflare API-quota exhaustion vector. No production caller of
        # this method was found anywhere in the codebase at the time of
        # this fix (grepped for real, not assumed) -- the elevation to a
        # service account is meant for an already-authorized internal
        # caller (an honeypot/abuse-detection integration, or an admin),
        # not a blanket bypass any RPC caller gets for free.
        if not (
            self.env.user.has_group("cloudflare.group_cloudflare_waf")
            or self.env.user.has_group("base.group_system")
            or self.env.user.is_service_account
        ):
            raise AccessError(_("You are not authorized to manage the Cloudflare WAF."))

        if not website_id:
            website_id = self.env["cloudflare.utils"].get_current_website_id()

        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_waf"
        )
        ban_env = self.env["cloudflare.ip.ban"].with_user(svc_uid).with_context(mail_notrack=True)
        return ban_env._execute_ban(
            ip_address, mode=mode, notes=notes, website_id=website_id
        )
