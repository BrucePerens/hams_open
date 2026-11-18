# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
from odoo import models, fields, api, _
from odoo.addons.distributed_redis_cache.redis_cache import notify_model_invalidation
from odoo.exceptions import UserError
import logging
from cryptography.fernet import InvalidToken
import hmac

from odoo.addons.zero_sudo.models.module_secrets import group_enabled, optional_store_secret, store_secret

# Group "cloudflare" of the host option hams_module_secrets_from_store: the API token and the Turnstile secret
# come from the secret store, host-wide, under these names, and the encrypted columns are cleared.
STORE_GROUP = "cloudflare"
STORE_NAMES = {
    "cloudflare_api_token": "cloudflare.api_token",
    "cloudflare_turnstile_secret": "cloudflare.turnstile_secret",
}
CRYPT_FIELDS = {
    "cloudflare_api_token": "cloudflare_api_token_crypt",
    "cloudflare_turnstile_secret": "cloudflare_turnstile_secret_crypt",
}

# Fields whose value _get_cloudflare_credentials() reads, directly or via the encrypted-field compute chain above it.
# That method is no longer cached in Redis, so the notify below only drops other cached results for the website model.
_CLOUDFLARE_CREDENTIAL_FIELDS = frozenset({
    "cloudflare_api_token",
    "cloudflare_api_token_crypt",
    "cloudflare_zone_id",
    "cloudflare_account_id",
    "cloudflare_turnstile_secret",
    "cloudflare_turnstile_secret_crypt",
})


# [@ANCHOR: cloudflare_website_website_cloudflare]
class WebsiteCloudflare(models.Model):
    _inherit = "website"

    # [@ANCHOR: cloudflare:COMM_website_write_busts_credential_cache]
    # Verified by [@ANCHOR: COMM_test_credential_write_busts_distributed_cache]
    def write(self, vals):
        result = super().write(vals)
        if _CLOUDFLARE_CREDENTIAL_FIELDS & vals.keys():
            notify_model_invalidation(self.env, "website")
        self._register_secrets_with_canary(vals)
        return result

    def _register_secrets_with_canary(self, vals):
        # [@ANCHOR: cloudflare:register_secrets_with_canary]
        """When the optional `hams_secret_canary` addon is installed, registers the fingerprint of a newly written API
        token or Turnstile secret so that its appearance in a response or a log line raises an alarm. Only a salted
        hash is kept. A value that is not a single token (too short, or has spaces) is skipped: saving never fails
        because of the canary."""
        if "hams.secret.canary" not in self.env:
            return
        for field, label in (("cloudflare_api_token", "cloudflare_api_token"),
                             ("cloudflare_turnstile_secret", "cloudflare_turnstile_secret")):
            value = vals.get(field)
            if value and value != "********":
                try:
                    self.env["hams.secret.canary"]._register_secret(label, value)
                except ValueError as error:
                    logging.getLogger(__name__).warning("Not registered with the secret canary: %s", error)

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
    cloudflare_purge_hosts = fields.Text(
        string="CF Hostnames in a Shared Zone",
        groups="base.group_system,cloudflare.group_cloudflare_purge,cloudflare.group_cloudflare_waf,cloudflare.group_cloudflare_tunnel",
        help="Only when the Zone ID above is a zone this website shares with other domains (it is served there as a "
        "Cloudflare for SaaS custom hostname): the host names it answers to, one per line or comma separated "
        "(hams.com, www.hams.com). Purge Everything then purges these host names only, never the whole zone.",
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

    # [@ANCHOR: cloudflare:COMM_website_purge_hosts]
    # Verified by [@ANCHOR: cloudflare:COMM_test_website_purge_hosts]
    def _cloudflare_purge_hosts(self):
        """The host names of cloudflare_purge_hosts, lower case, each once, in the order written. Empty: this
        website has its zone to itself and Purge Everything may purge the zone."""
        self.ensure_one()
        names = (self.cloudflare_purge_hosts or "").replace(",", "\n").split()
        return list(dict.fromkeys(name.lower() for name in names))

    # [@ANCHOR: cloudflare:COMM_website_shared_zone_refusal]
    # Verified by [@ANCHOR: cloudflare:COMM_test_shared_zone_refusal]
    def _cloudflare_shared_zone_refusal(self):
        """Why a zone-wide change (WAF, cache rules, zone settings, rate limits) must not be pushed for this website,
        or "" when it may. A website that lists cloudflare_purge_hosts shares its zone with other domains: those
        pushes replace a whole ruleset or setting of the zone, which would overwrite what the other domains rely on."""
        self.ensure_one()
        if not self._cloudflare_purge_hosts():
            return ""
        return _(
            "REFUSED: %(site)s shares its Cloudflare zone with other domains (its hostnames are listed in Settings > "
            "Cloudflare), and this change would replace a whole ruleset or setting of that zone. Make the change at "
            "Cloudflare, scoped to this website's host names.", site=self.name)

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
            company = self.company_id or self.env.company
            reason = self.env["cloudflare.tenant.key"]._fernet_failure(company)
            if reason == "malformed":
                raise UserError(_(
                    "Cannot store this Cloudflare secret: the deployment-wide "
                    "cryptographic secret is malformed (not a valid Fernet key)."
                ))
            if reason == "unwrap":
                raise UserError(_(
                    "Cannot store this Cloudflare secret: the stored tenant key "
                    "cannot be unwrapped with the deployment-wide cryptographic "
                    "secret (has the secret changed?)."
                ))
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
            # The actual, only exception Fernet.decrypt() raises for a corrupted/
            # tampered/wrong-key ciphertext is `cryptography.fernet.
            # InvalidToken`, a direct Exception subclass -- NOT a ValueError
            # (independently confirmed against the installed `cryptography`
            # package: decrypt() on malformed/garbage/empty input always
            # raises InvalidToken, never ValueError). Catching only
            # `ValueError` here would miss the real failure mode this handler
            # exists for -- e.g. after rotating the shared
            # `daemon.key.registry` "cloudflare_encryption_key" value, every
            # previously-encrypted token would raise InvalidToken uncaught
            # straight out of a compute field (_compute_cf_api_token /
            # _compute_cf_turnstile_secret), crashing the Settings form
            # instead of showing the intended "***ERROR***" marker.
            logging.getLogger(__name__).warning("Encryption/Decryption error: %s", e)
            return "***ERROR***" if decrypt else False

    # [@ANCHOR: cloudflare:COMM_compute_encrypted_field]
    def _compute_encrypted_field(self, plain_field, crypt_field):
        if group_enabled(STORE_GROUP):
            value = optional_store_secret(STORE_NAMES[plain_field]) or False
            for rec in self:
                setattr(rec, plain_field, value)
            return
        for rec in self:
            setattr(rec, plain_field, rec._crypt_field(getattr(rec, crypt_field), decrypt=True))

    # [@ANCHOR: cloudflare:COMM_inverse_encrypted_field]
    def _inverse_encrypted_field(self, plain_field, crypt_field):
        if group_enabled(STORE_GROUP):
            # Store only: a typed secret is refused (put it in the store); clearing is a no-op, the store owns it.
            if any(getattr(rec, plain_field) for rec in self):
                raise UserError(_(
                    "The Cloudflare secrets are served from the secret store on this host "
                    "(hams_module_secrets_from_store). Put the new value in the store, name %(name)s.",
                    name=STORE_NAMES[plain_field]))
            return
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
    # Deliberately not @distributed_cache(): that stores the result in Redis for 24 hours, and this returns the API token.
    def _get_cloudflare_credentials(self, override_svc_uid=None):
        """
        Returns the API Token and Zone ID for this specific website.
        """
        if override_svc_uid:
            self = self.with_user(override_svc_uid)
        self.ensure_one()
        token = self.cloudflare_api_token
        zone = self.cloudflare_zone_id
        # On a genuine decrypt failure, _crypt_field()
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

    # [@ANCHOR: cloudflare:store_check_secrets]
    @api.model
    def _check_cloudflare_secrets_in_store(self):
        """With the group on: every secret a website still has an encrypted copy of must be in the store, else
        SecretNotFound (names only). A website without a secret needs nothing."""
        if not group_enabled(STORE_GROUP):
            return
        websites = self.search([], limit=10000)
        for plain, crypt in CRYPT_FIELDS.items():
            if any(websites.mapped(crypt)):
                store_secret(STORE_NAMES[plain])

    # [@ANCHOR: cloudflare:store_register_hook]
    def _register_hook(self):
        """A host that lists the group but has an incomplete store refuses to load the registry."""
        super()._register_hook()
        self._check_cloudflare_secrets_in_store()

    # [@ANCHOR: cloudflare:store_clear_from_database]
    @api.model
    def _hams_cloudflare_clear_secrets_from_database(self):
        """Rollout step after the flip: empties the encrypted columns. Refuses (writing nothing) unless the group
        is on, the store holds each secret that has an encrypted copy, and every website's copy equals the store
        value (the store is host-wide, so a website with a different token would otherwise lose it). Returns the
        field names it cleared (names only)."""
        if not group_enabled(STORE_GROUP):
            raise UserError(_("List the group %s in hams_module_secrets_from_store first.", STORE_GROUP))
        websites = self.search([], limit=10000)
        cleared = []
        for plain, crypt in CRYPT_FIELDS.items():
            holders = websites.filtered(crypt)
            if not holders:
                continue
            wanted = store_secret(STORE_NAMES[plain]).encode("utf-8")
            for site in holders:
                have = site._crypt_field(site[crypt], decrypt=True).encode("utf-8")
                if not hmac.compare_digest(have, wanted):
                    raise UserError(_(
                        "Website %(site)s holds a different %(field)s than the secret store; the store is "
                        "host-wide. Nothing was cleared.", site=site.name, field=plain))
            holders.write({crypt: False})
            cleared.append(crypt)
        return cleared
