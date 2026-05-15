# -*- coding: utf-8 -*-
from odoo import models, api
from odoo.http import request


class CloudflareWAF(models.AbstractModel):
    _name = "cloudflare.waf"
    _description = "Cloudflare WAF Interface"

    @api.model
    def ban_ip(
        # [@ANCHOR: cf_ban_ip_api]
        self,
        ip_address,
        mode="block",
        duration=3600,
        notes="Honeypot Triggered",
        website_id=None,
    ):
        if not website_id:

            try:
                if request and getattr(request, "website", False):
                    website_id = request.website.id
                else:
                    website_id = self.env["website"].get_current_website().id
            except RuntimeError:
                website_id = self.env["website"].get_current_website().id

        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_waf"
        )
        ban_env = self.env["cloudflare.ip.ban"].with_user(svc_uid)
        return ban_env._execute_ban(
            ip_address, mode=mode, notes=notes, website_id=website_id
        )
