# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0.
from odoo import models, fields, api
from odoo.addons.distributed_redis_cache.redis_cache import distributed_cache
import os
import logging
from cryptography.fernet import Fernet


class WebsiteCloudflare(models.Model):
    _inherit = "website"

    cloudflare_ip_ban_ids = fields.One2many("cloudflare.ip.ban", "website_id")
    cloudflare_tunnel_ids = fields.One2many("cloudflare.tunnel", "website_id")
    cloudflare_waf_rule_ids = fields.One2many("cloudflare.waf.rule", "website_id")
    cloudflare_purge_queue_ids = fields.One2many("cloudflare.purge.queue", "website_id")

    cloudflare_api_token_crypt = fields.Char(
        string="Encrypted CF API Token",
        groups="base.group_system,cloudflare.group_cloudflare_purge,cloudflare.group_cloudflare_waf,cloudflare.group_cloudflare_tunnel",
    )
    cloudflare_api_token = fields.Char(
        string="CF API Token",
        compute="_compute_cf_api_token",
        inverse="_inverse_cf_api_token",
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
    cloudflare_turnstile_secret_crypt = fields.Char(
        string="Encrypted Turnstile Secret",
        groups="base.group_system,cloudflare.group_cloudflare_purge,cloudflare.group_cloudflare_waf,cloudflare.group_cloudflare_tunnel",
    )
    cloudflare_turnstile_secret = fields.Char(
        string="Turnstile Secret",
        compute="_compute_cf_turnstile_secret",
        inverse="_inverse_cf_turnstile_secret",
        groups="base.group_system,cloudflare.group_cloudflare_purge,cloudflare.group_cloudflare_waf,cloudflare.group_cloudflare_tunnel",
        help="Secret key for Cloudflare Turnstile integration.",
    )

    def _get_fernet(self):
        key = os.environ.get("HAMS_CRYPTO_KEY")
        if not key:
            return None
        return Fernet(key.encode("utf-8"))

    def _crypt_field(self, value, decrypt=False):
        f = self._get_fernet()
        if not f or not value:
            return False
        try:
            if decrypt:
                return f.decrypt(value.encode("utf-8")).decode("utf-8")
            else:
                return f.encrypt(value.encode("utf-8")).decode("utf-8")
        except ValueError as e:
            logging.getLogger(__name__).warning("Encryption/Decryption error: %s", e)
            return "***ERROR***" if decrypt else False

    def _compute_encrypted_field(self, plain_field, crypt_field):
        for rec in self:
            setattr(rec, plain_field, rec._crypt_field(getattr(rec, crypt_field), decrypt=True))

    def _inverse_encrypted_field(self, plain_field, crypt_field):
        for rec in self:
            setattr(rec, crypt_field, rec._crypt_field(getattr(rec, plain_field)))

    @api.depends("cloudflare_api_token_crypt")
    def _compute_cf_api_token(self):
        self._compute_encrypted_field("cloudflare_api_token", "cloudflare_api_token_crypt")

    def _inverse_cf_api_token(self):
        self._inverse_encrypted_field("cloudflare_api_token", "cloudflare_api_token_crypt")

    @api.depends("cloudflare_turnstile_secret_crypt")
    def _compute_cf_turnstile_secret(self):
        self._compute_encrypted_field("cloudflare_turnstile_secret", "cloudflare_turnstile_secret_crypt")

    def _inverse_cf_turnstile_secret(self):
        self._inverse_encrypted_field("cloudflare_turnstile_secret", "cloudflare_turnstile_secret_crypt")

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
