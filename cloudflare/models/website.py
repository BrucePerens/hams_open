# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0.
from odoo import models, fields
from odoo.addons.distributed_redis_cache.redis_cache import distributed_cache


class WebsiteCloudflare(models.Model):
    _inherit = "website"

    cloudflare_api_token = fields.Char(
        string="CF API Token",
        groups="base.group_system,cloudflare.group_cloudflare_purge,cloudflare.group_cloudflare_waf,cloudflare.group_cloudflare_tunnel",
    )
    cloudflare_zone_id = fields.Char(
        string="CF Zone ID",
        groups="base.group_system,cloudflare.group_cloudflare_purge,cloudflare.group_cloudflare_waf,cloudflare.group_cloudflare_tunnel",
    )
    cloudflare_account_id = fields.Char(
        string="CF Account ID",
        groups="base.group_system,cloudflare.group_cloudflare_purge,cloudflare.group_cloudflare_waf,cloudflare.group_cloudflare_tunnel",
    )
    cloudflare_turnstile_secret = fields.Char(
        string="Turnstile Secret",
        groups="base.group_system,cloudflare.group_cloudflare_purge,cloudflare.group_cloudflare_waf,cloudflare.group_cloudflare_tunnel",
    )

    @distributed_cache()
    def _get_cloudflare_credentials(self):
        """
        Returns the API Token and Zone ID for this specific website.
        """
        self.ensure_one()
        token = self.cloudflare_api_token
        zone = self.cloudflare_zone_id
        return token, zone
