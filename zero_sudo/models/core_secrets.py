# -*- coding: utf-8 -*-
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Odoo core's own secrets served from the secret store, behind a per-host flag.

Odoo core reads two secrets from `ir.config_parameter`, so they sit in every database dump:

- `database.secret`: CSRF tokens (odoo/http.py), `odoo.tools.misc.hmac` (portal, mail and mass-mailing
  links), and the session token, which `res.users` reads with its own SQL rather than `get_param`.
- `mail.web_push_vapid_private_key`: signs every Web Push message (mail_push.py, mail_thread.py).

With the Odoo configuration option `hams_core_secrets_from_store = True` (odoo.conf, `[options]`), the two
are read from the secret store (daemon/secret_store.py, `default_store()`) under the same names, and the
database row keeps only MOVED_MARKER. Without the option, or with it false, nothing here changes Odoo's
behaviour: the values come from `ir.config_parameter` as before, and the store is never consulted, so a
host without a store keeps working. The flag is a host setting, not a database setting, on purpose: a
database row that could switch the source back would let a database write choose the secret.

Rollout and rollback: hams_com docs/runbooks/CORE_SECRETS_FROM_STORE.md. Story:
zero_sudo/docs/stories/secret_store_tier_a.md ("Odoo core's secrets").
"""

import hmac
import logging

from odoo import api, models, tools
from odoo.exceptions import UserError

from odoo.addons.zero_sudo.daemon import secret_store

_logger = logging.getLogger(__name__)

FLAG_OPTION = "hams_core_secrets_from_store"
DATABASE_SECRET = "database.secret"
VAPID_PRIVATE_KEY = "mail.web_push_vapid_private_key"
VAPID_PUBLIC_KEY = "mail.web_push_vapid_public_key"
CORE_SECRET_KEYS = (DATABASE_SECRET, VAPID_PRIVATE_KEY)
# What the database row holds once the secret lives in the store. Not a secret; never a valid secret either
# (a value the store would serve is never this text, see clear_core_secrets_from_database()).
MOVED_MARKER = "moved-to-secret-store"
_TRUE = ("1", "true", "yes", "on")
_FALSE = ("", "0", "false", "no", "off")


# [@ANCHOR: zero_sudo:core_secrets_parse_flag]
def parse_flag(value):
    """True or False for the option's text (None, a bool, or true/false/yes/no/on/off/1/0, any case).
    Anything else is a ValueError: a typo must not silently pick the database."""
    if value is None or isinstance(value, bool):
        return bool(value)
    text = str(value).strip().lower()
    if text in _TRUE:
        return True
    if text in _FALSE:
        return False
    raise ValueError(f"{FLAG_OPTION} = {value!r}: expected True or False")


# [@ANCHOR: zero_sudo:core_secrets_from_store]
def core_secrets_from_store(config=None):
    """True when this Odoo process reads Odoo core's secrets from the store: the odoo.conf option, read from
    `config` (a mapping with `get`; Odoo's `tools.config` when None)."""
    config = tools.config if config is None else config
    return parse_flag(config.get(FLAG_OPTION))


# [@ANCHOR: zero_sudo:core_secrets_store]
def core_secret_store():
    """The process's secret store (a separate function so tests can give it a store of their own)."""
    return secret_store.default_store()


# [@ANCHOR: zero_sudo:core_secrets_store_value]
def store_value(key):
    """The store's value for a core secret key; SecretNotFound when the store does not hold it."""
    return core_secret_store().get(key)


# [@ANCHOR: zero_sudo:core_secrets_optional_store_value]
def _optional_store_value(key):
    """The store's value, or None when the store does not hold it."""
    try:
        return store_value(key)
    except secret_store.SecretNotFound:
        return None


# [@ANCHOR: zero_sudo_core_secrets_ir_config_parameter_core_secrets]
class IrConfigParameterCoreSecrets(models.Model):
    _inherit = "ir.config_parameter"

    # [@ANCHOR: zero_sudo:core_secrets_get_param]
    @api.model
    def _get_param(self, key):
        """With the flag on, `database.secret` comes from the store (SecretNotFound when it is missing: Odoo
        cannot sign anything without it) and the VAPID private key too (None when missing, which core reads
        as "Web Push is not set up"; the registry check refuses a public key without its private key). The
        values are not put in the ormcache, so no copy outlives a change in the store beyond this call.
        Every other key, and every key with the flag off, is Odoo's own `_get_param`."""
        if key in CORE_SECRET_KEYS and core_secrets_from_store():
            if key == DATABASE_SECRET:
                return store_value(key)
            return _optional_store_value(key)
        return super()._get_param(key)

    # [@ANCHOR: zero_sudo:core_secrets_db_value]
    @api.model
    def _core_secret_db_value(self, key, value):
        """What the database row gets when `value` is written to core secret `key` with the flag on.
        The real secret never goes back into the database unless the restore context is set."""
        if self.env.context.get("hams_core_secret_restore"):
            return value
        if value in (False, None) or value == MOVED_MARKER:
            return value
        if key == DATABASE_SECRET:
            # Odoo writes a fresh one when it creates or duplicates a database (`init`). With the flag on, the
            # host's store value is what every database on this host signs with; the new value is not kept.
            _logger.warning(
                "database.secret is served from the secret store (%s); the database keeps only the marker",
                FLAG_OPTION,
            )
            return MOVED_MARKER
        current = _optional_store_value(key)
        if current is not None and hmac.compare_digest(str(value).encode("utf-8", "replace"), current.encode("utf-8", "replace")):
            return MOVED_MARKER
        # A new VAPID key pair (mail.push.device generates one when the public key is missing): the public key
        # would land in the database while the private key is dropped, and every push would fail its
        # signature. Refuse so the new pair goes into the store first.
        raise UserError(self.env._(
            "%(key)s is served from the secret store on this host (%(flag)s). Put the new value in the secret "
            "store first; see docs/runbooks/CORE_SECRETS_FROM_STORE.md.",
            key=key, flag=FLAG_OPTION,
        ))

    # [@ANCHOR: zero_sudo:core_secrets_create]
    @api.model_create_multi
    def create(self, vals_list):
        if core_secrets_from_store():
            vals_list = [
                dict(vals, value=self._core_secret_db_value(vals["key"], vals.get("value")))
                if vals.get("key") in CORE_SECRET_KEYS and "value" in vals else vals
                for vals in vals_list
            ]
        return super().create(vals_list)

    # [@ANCHOR: zero_sudo:core_secrets_write]
    def write(self, vals):
        if "value" in vals and core_secrets_from_store():
            secret_rows = self.filtered(lambda p: p.key in CORE_SECRET_KEYS)
            if secret_rows:
                others = self - secret_rows
                result = True
                if others:
                    result = super(IrConfigParameterCoreSecrets, others).write(vals)
                for row in secret_rows:
                    row_vals = dict(vals, value=self._core_secret_db_value(row.key, vals["value"]))
                    result = super(IrConfigParameterCoreSecrets, row).write(row_vals) and result
                return result
        return super().write(vals)

    # [@ANCHOR: zero_sudo:core_secrets_check_store]
    @api.model
    def _check_core_secrets_in_store(self):
        """With the flag on: the store holds `database.secret`, and the VAPID private key whenever the
        database has a VAPID public key. Raises SecretNotFound naming the missing one (never a value)."""
        if not core_secrets_from_store():
            return
        store_value(DATABASE_SECRET)
        if super()._get_param(VAPID_PUBLIC_KEY):
            store_value(VAPID_PRIVATE_KEY)

    # [@ANCHOR: zero_sudo:core_secrets_register_hook]
    def _register_hook(self):
        """A host with the flag on and an incomplete store refuses to load the registry, instead of serving
        pages whose CSRF check or push signature fails later. With the flag off the store is not read."""
        super()._register_hook()
        self._check_core_secrets_in_store()

    # [@ANCHOR: zero_sudo:core_secrets_clear_from_database]
    @api.model
    def _hams_core_secrets_clear_from_database(self):
        """Rollout step after the flip: replace the database copies with MOVED_MARKER. Refuses (and writes
        nothing) unless the flag is on and the store holds every key the database still has a value for, so
        it cannot throw away the only copy. Returns the keys it cleared (names only)."""
        if not core_secrets_from_store():
            raise UserError(self.env._("Turn %(flag)s on before clearing the database copies.", flag=FLAG_OPTION))
        rows = self.search([("key", "in", list(CORE_SECRET_KEYS))], limit=len(CORE_SECRET_KEYS))
        pending = rows.filtered(lambda p: p.value and p.value != MOVED_MARKER)
        for row in pending:
            store_value(row.key)
        for row in pending:
            row.write({"value": MOVED_MARKER})
        return sorted(pending.mapped("key"))

    # [@ANCHOR: zero_sudo:core_secrets_restore_to_database]
    @api.model
    def _hams_core_secrets_restore_to_database(self):
        """Rollback step before the flag goes off: put the store's values back into the database rows, so
        the flag-off host signs with the same secret (sessions, CSRF tokens and push subscriptions stay
        valid). Keys the store does not hold are left alone. Returns the keys it restored (names only)."""
        restored = []
        for key in CORE_SECRET_KEYS:
            value = _optional_store_value(key)
            if value is None:
                continue
            self.with_context(hams_core_secret_restore=True).set_param(key, value)
            restored.append(key)
        return restored


# [@ANCHOR: zero_sudo_core_secrets_res_users_core_secrets]
class ResUsersCoreSecrets(models.Model):
    _inherit = "res.users"

    # [@ANCHOR: zero_sudo:core_secrets_session_token_values]
    def _session_token_get_values(self):
        """Core reads `database.secret` for the session token with its own SQL, past `get_param`. With the
        flag on, its value in the token key is the store's, so a database row holding MOVED_MARKER does not
        sign sessions, and a store value equal to the old row keeps every open session valid."""
        values = super()._session_token_get_values()
        if not values or not core_secrets_from_store():
            return values
        secret = store_value(DATABASE_SECRET)
        return tuple((name, secret if name == "database_secret" else value) for name, value in values)
