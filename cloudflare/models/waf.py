# -*- coding: utf-8 -*-
from odoo import models, api


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
            website_id = self.env["cloudflare.utils"].get_current_website_id()

        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_waf"
        )
        ban_env = self.env["cloudflare.ip.ban"].with_user(svc_uid)
        return ban_env._execute_ban(
            ip_address, mode=mode, notes=notes, website_id=website_id
        )
