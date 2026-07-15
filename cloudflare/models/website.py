# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0.
from odoo import models, fields
from odoo.addons.distributed_redis_cache.redis_cache import distributed_cache


class WebsiteCloudflare(models.Model):
    _inherit = "website"

    cloudflare_ip_ban_ids = fields.One2many("cloudflare.ip.ban", "website_id")
    cloudflare_tunnel_ids = fields.One2many("cloudflare.tunnel", "website_id")
    cloudflare_waf_rule_ids = fields.One2many("cloudflare.waf.rule", "website_id")
    cloudflare_purge_queue_ids = fields.One2many("cloudflare.purge.queue", "website_id")

    cloudflare_api_token = fields.Char(
        string="CF API Token",
        groups="base.group_system,cloudflare.group_cloudflare_purge,cloudflare.group_cloudflare_waf,cloudflare.group_cloudflare_tunnel",
        help="Required to authenticate with Cloudflare API.",
    )
    cloudflare_zone_id = fields.Char(
        string="CF Zone ID",
        groups="base.group_system,cloudflare.group_cloudflare_purge,cloudflare.group_cloudflare_waf,cloudflare.group_cloudflare_tunnel",
        help="The Zone ID of your domain on Cloudflare.",
    )
    cloudflare_account_id = fields.Char(
        string="CF Account ID",
        groups="base.group_system,cloudflare.group_cloudflare_purge,cloudflare.group_cloudflare_waf,cloudflare.group_cloudflare_tunnel",
        help="The Account ID associated with your Cloudflare account.",
    )
    cloudflare_turnstile_secret = fields.Char(
        string="Turnstile Secret",
        groups="base.group_system,cloudflare.group_cloudflare_purge,cloudflare.group_cloudflare_waf,cloudflare.group_cloudflare_tunnel",
        help="Secret key for Cloudflare Turnstile integration.",
    )

    @distributed_cache()
    def _get_cloudflare_credentials(self, override_svc_uid=None):
        """
        Returns the API Token and Zone ID for this specific website.
        """
        if override_svc_uid:
            self = self.with_user(override_svc_uid)
        self.ensure_one()
        token = self.cloudflare_api_token
        zone = self.cloudflare_zone_id
        return token, zone
