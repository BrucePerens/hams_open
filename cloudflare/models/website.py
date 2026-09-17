# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
from odoo import models, fields, api
from odoo.addons.distributed_redis_cache.redis_cache import distributed_cache
import logging
from cryptography.fernet import Fernet, InvalidToken


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

    # [@ANCHOR: cloudflare:COMM_get_fernet]
    def _get_fernet(self):
        # Bug fix (night-watch, 2026-09-17): this used to search
        # `daemon.key.registry` for a `cloudflare_encryption_key` row and
        # read `.value` off it. That model has no `value` field at all --
        # it's a write-only-to-disk API-key rotation registry for daemons
        # (name/user_id/env_file_path/company_id/last_rotated), not a
        # key-value secret store. Since no such row was ever created in any
        # environment (there is nowhere to create one from), the search
        # always returned empty and this always fell through to `None`,
        # silently disabling Turnstile/API-token encryption everywhere --
        # `test_02_turnstile_secret_fetch` only ever looked like it passed
        # because it read back its own uninvalidated ORM write-buffer cache
        # within the same test method (see cloudflare-three-preexisting-
        # test-failures-035f2dfc.md). Use the same project-wide crypto
        # secret resolver `ham_logbook`'s analogous _get_fernet_cipher()
        # and user_websites already use (`zero_sudo` is already a manifest
        # dependency of this module). The per-company-key isolation this
        # method used to gesture at in a comment doesn't apply yet -- there
        # is only one company in any deployment today -- and is tracked
        # separately in night_shift_todo/low/cloudflare-per-tenant-
        # encryption-key-if-multitenancy-lands.md if that ever changes.
        key = self.env["zero_sudo.security.utils"]._get_crypto_secret()
        if not key:
            return None
        # Bug fix (bug-hunt, review_tier 1, 2026-09-09): a corrupted/malformed
        # key row (wrong length, not valid url-safe base64) previously raised
        # ValueError straight out of this method, uncaught -- crashing any
        # compute (_compute_cf_api_token/_compute_cf_turnstile_secret) that
        # calls it, since those are @api.depends compute methods with no
        # try/except of their own. Fail closed to "no Fernet available"
        # instead, which _crypt_field below already treats the same as "no
        # key configured" (falls through to its own False/`***ERROR***`
        # handling rather than raising).
        try:
            return Fernet(key.encode("utf-8"))
        except ValueError as e:
            logging.getLogger(__name__).warning(
                "Cloudflare encryption key is malformed: %s", e
            )
            return None

    # [@ANCHOR: cloudflare:COMM_crypt_field]
    def _crypt_field(self, value, decrypt=False):
        f = self._get_fernet()
        if not f or not value:
            return False
        try:
            if decrypt:
                return f.decrypt(value.encode("utf-8")).decode("utf-8")
            else:
                return f.encrypt(value.encode("utf-8")).decode("utf-8")
        except (ValueError, InvalidToken) as e:
            # Bug fix (bug-hunt, review_tier 1, 2026-09-09, bug class 20): the
            # actual, only exception Fernet.decrypt() raises for a corrupted/
            # tampered/wrong-key ciphertext is `cryptography.fernet.
            # InvalidToken`, a direct Exception subclass -- NOT a ValueError
            # (independently confirmed against the installed `cryptography`
            # package: decrypt() on malformed/garbage/empty input always
            # raises InvalidToken, never ValueError). The original
            # `except ValueError` here never caught the real failure mode it
            # exists to handle -- e.g. after rotating the shared
            # `daemon.key.registry` "cloudflare_encryption_key" value, every
            # previously-encrypted token would raise InvalidToken uncaught
            # straight out of a compute field (_compute_cf_api_token /
            # _compute_cf_turnstile_secret), crashing the Settings form
            # instead of showing the intended "***ERROR***" marker.
            logging.getLogger(__name__).warning("Encryption/Decryption error: %s", e)
            return "***ERROR***" if decrypt else False

    # [@ANCHOR: cloudflare:COMM_compute_encrypted_field]
    def _compute_encrypted_field(self, plain_field, crypt_field):
        for rec in self:
            setattr(rec, plain_field, rec._crypt_field(getattr(rec, crypt_field), decrypt=True))

    # [@ANCHOR: cloudflare:COMM_inverse_encrypted_field]
    def _inverse_encrypted_field(self, plain_field, crypt_field):
        for rec in self:
            setattr(rec, crypt_field, rec._crypt_field(getattr(rec, plain_field)))

    # [@ANCHOR: cloudflare:COMM_compute_cf_api_token]
    @api.depends("cloudflare_api_token_crypt")
    def _compute_cf_api_token(self):
        self._compute_encrypted_field("cloudflare_api_token", "cloudflare_api_token_crypt")

    # [@ANCHOR: cloudflare:COMM_inverse_cf_api_token]
    def _inverse_cf_api_token(self):
        self._inverse_encrypted_field("cloudflare_api_token", "cloudflare_api_token_crypt")

    # [@ANCHOR: cloudflare:COMM_compute_cf_turnstile_secret]
    @api.depends("cloudflare_turnstile_secret_crypt")
    def _compute_cf_turnstile_secret(self):
        self._compute_encrypted_field("cloudflare_turnstile_secret", "cloudflare_turnstile_secret_crypt")

    # [@ANCHOR: cloudflare:COMM_inverse_cf_turnstile_secret]
    def _inverse_cf_turnstile_secret(self):
        self._inverse_encrypted_field("cloudflare_turnstile_secret", "cloudflare_turnstile_secret_crypt")

    # [@ANCHOR: cloudflare:COMM_get_cloudflare_credentials]
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
        # bug-hunt (2026-09-13): on a genuine decrypt failure, _crypt_field()
        # returns the literal string "***ERROR***" -- a truthy value, and a
        # deliberate, established cross-module UI convention (backup_
        # management's own crypt fields use the identical sentinel), not
        # something to change here. But every caller of this function gates
        # on plain truthiness (`if token and zone_id:`, `if not token:`), so
        # without this check they'd proceed to call the real Cloudflare API
        # with the literal string "***ERROR***" as the bearer token on a
        # decrypt failure, instead of failing fast locally with a clear
        # error. Centralized here (this function's own real callers all go
        # through it) rather than fixed at each of the dozen-plus call
        # sites individually.
        if token == "***ERROR***":
            token = False
        return token, zone
