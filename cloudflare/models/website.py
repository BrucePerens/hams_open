# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
from odoo import models, fields, api, _
from odoo.addons.distributed_redis_cache.redis_cache import (
    distributed_cache,
    notify_model_invalidation,
)
from odoo.exceptions import UserError
import logging
from cryptography.fernet import InvalidToken

# Fields whose value _get_cloudflare_credentials() (a @distributed_cache()'d
# method) reads, directly or via the encrypted-field compute chain above it.
# A plain website.write() bypasses that decorator entirely -- it has no idea
# these particular fields feed a cross-worker Redis-backed cache with a 24h
# TTL (see redis_cache.py's `r.setex(cache_key, 86400, ...)`), so without the
# explicit bust below, a credential change here is invisible to every OTHER
# worker (and to a fresh `odoo shell` once Redis is reachable) for up to a
# day, or until the process restarts -- exactly the kind of "looks like it
# worked, silently didn't" failure this module's own write path already got
# fixed for once (see _crypt_field's history above).
_CLOUDFLARE_CREDENTIAL_FIELDS = frozenset({
    "cloudflare_api_token",
    "cloudflare_api_token_crypt",
    "cloudflare_zone_id",
    "cloudflare_account_id",
    "cloudflare_turnstile_secret",
    "cloudflare_turnstile_secret_crypt",
})


class WebsiteCloudflare(models.Model):
    _inherit = "website"

    # [@ANCHOR: cloudflare:COMM_website_write_busts_credential_cache]
    def write(self, vals):
        result = super().write(vals)
        if _CLOUDFLARE_CREDENTIAL_FIELDS & vals.keys():
            notify_model_invalidation(self.env, "website")
        return result

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
        # test-failures-035f2dfc.md).
        #
        # Design update (night-watch, 2026-09-17, per Bruce -- "this module
        # is for the open source community. Assume some of them will be
        # multi-tenant. Design it to work for them."): rather than one
        # shared Fernet key for every company on the instance, each
        # company gets its own independently-generated key, envelope-
        # encrypted (see cloudflare.tenant.key) under the single
        # deployment-wide crypto secret. A compromised or rotated
        # deployment secret, or a leaked key for one company, never
        # exposes another company's Turnstile secret or API token.
        self.ensure_one()
        company = self.company_id or self.env.company
        return self.env["cloudflare.tenant.key"]._get_or_create_fernet(company)

    # [@ANCHOR: cloudflare:COMM_crypt_field]
    def _crypt_field(self, value, decrypt=False):
        f = self._get_fernet()
        if not value:
            return False
        if not f:
            # Bug fix (2026-09-22, per Bruce -- "things should not fail
            # silently"): this used to fold "no deployment-wide crypto
            # secret configured" into the same `return False` as "no value
            # to encrypt". `_get_or_create_fernet` already logs the real
            # cause loudly (ERROR), but that log line was the only trace --
            # `website.write({"cloudflare_api_token": ...})` raised nothing
            # and looked like it succeeded while silently discarding the
            # value, and a later read of an already-stored token looked
            # identical to "never configured" instead of "can't decrypt
            # right now". Encrypt (the write path) now fails loudly since
            # there is a real value the caller expects stored. Decrypt (the
            # read path) uses the same "***ERROR***" sentinel already used
            # a few lines below for a corrupted/rotated key (InvalidToken),
            # which is the established convention this module's callers
            # (e.g. `_get_cloudflare_credentials`) already know to check
            # for, rather than inventing a second distinct failure shape.
            if decrypt:
                return "***ERROR***"
            raise UserError(_(
                "Cannot store this Cloudflare secret: no deployment-wide "
                "cryptographic secret is configured (HAMS_CRYPTO_KEY, "
                "/var/lib/odoo/hams_crypto.secret, or a non-default "
                "admin_passwd). Configure one before saving Cloudflare "
                "credentials."
            ))
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
