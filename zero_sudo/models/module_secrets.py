# -*- coding: utf-8 -*-
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Module secrets (Stripe, AI provider keys, ...) served from the secret store, one group at a time.

The host option `hams_module_secrets_from_store` (odoo.conf, `[options]`) lists the groups whose secrets
come from the store, for example `stripe,ai`. A group that is not listed behaves exactly as before: the
value comes from `ir.config_parameter` and the store is never read. A listed group is store only: the
`ir.config_parameter` rows of its keys hold MOVED_MARKER (never a secret), a write of any other value is
refused, and a host whose store lacks a key the database still has a row for refuses to load the registry
(fail fast, never a silent fall back to the database). The option is a host setting on purpose, as for the
core secrets (core_secrets.py): a database row must not be able to switch the source back.

Groups that keep a secret in a model or settings field (hams_s3, cloudflare, mail) use `group_enabled()`
and `store_secret()` from this file and have their own tests. Story:
zero_sudo/docs/stories/secret_store_tier_a.md ("Module secrets").
"""

import hmac
import logging

from odoo import api, models, tools
from odoo.exceptions import UserError

from odoo.addons.zero_sudo.daemon import secret_store
from odoo.addons.zero_sudo.models.core_secrets import MOVED_MARKER

_logger = logging.getLogger(__name__)

FLAG_OPTION = "hams_module_secrets_from_store"
# group -> the ir.config_parameter keys (also the store names) it moves.
PARAM_GROUPS = {
    "stripe": ("stripe.secret_key", "stripe.webhook_secret"),
    "ai": ("gemini.api_key", "greyline.api_key", "greyline.password"),
}
# Groups whose secrets are not ir.config_parameter keys; the owning module reads the store itself.
FIELD_GROUPS = ("cloudflare",)
GROUPS = tuple(PARAM_GROUPS) + tuple(FIELD_GROUPS)


# [@ANCHOR: zero_sudo:module_secrets_enabled_groups]
def enabled_groups(config=None):
    """The set of groups the host option lists (a comma separated text, any case, blanks ignored; empty or
    unset is no group). A name that is not a known group is a ValueError: a typo must not silently pick the
    database for a secret the operator thinks is in the store."""
    config = tools.config if config is None else config
    value = config.get(FLAG_OPTION)
    if value is None or isinstance(value, bool) and not value:
        return frozenset()
    groups = frozenset(part.strip().lower() for part in str(value).split(",") if part.strip())
    unknown = sorted(groups - set(GROUPS))
    if unknown:
        raise ValueError(f"{FLAG_OPTION} names unknown group(s) {unknown}; known: {sorted(GROUPS)}")
    return groups


# [@ANCHOR: zero_sudo:module_secrets_group_enabled]
def group_enabled(group, config=None):
    """True when `group` is listed in the host option."""
    return group in enabled_groups(config)


# [@ANCHOR: zero_sudo:module_secrets_store]
def module_secret_store():
    """The process's secret store (a separate function so tests can give it a store of their own)."""
    return secret_store.default_store()


# [@ANCHOR: zero_sudo:module_secrets_store_secret]
def store_secret(name):
    """The store's value for `name`; SecretNotFound when the store does not hold it."""
    return module_secret_store().get(name)


# [@ANCHOR: zero_sudo:module_secrets_optional_store_secret]
def optional_store_secret(name):
    """The store's value, or None when the store does not hold it."""
    try:
        return store_secret(name)
    except secret_store.SecretNotFound:
        return None


# [@ANCHOR: zero_sudo:module_secrets_param_keys]
def param_keys(config=None):
    """The ir.config_parameter keys served from the store on this host."""
    return tuple(key for group in sorted(enabled_groups(config)) for key in PARAM_GROUPS.get(group, ()))


# [@ANCHOR: zero_sudo_module_secrets_ir_config_parameter_module_secrets]
class IrConfigParameterModuleSecrets(models.Model):
    _inherit = "ir.config_parameter"

    # [@ANCHOR: zero_sudo:module_secrets_get_param]
    @api.model
    def _get_param(self, key):
        """A key of an enabled group comes from the store (None when the store does not hold it, which the
        module reads as "not configured", as it does for an empty row). Never put in the ormcache. Every other
        key, and every key of a group that is not enabled, is Odoo's own `_get_param`."""
        if key in param_keys():
            return optional_store_secret(key)
        return super()._get_param(key)

    # [@ANCHOR: zero_sudo:module_secrets_db_value]
    @api.model
    def _module_secret_db_value(self, key, value):
        """What the database row gets when `value` is written to a store-served key: the marker for the
        value the store already holds, the empty values unchanged, an error for anything else (the real
        secret must reach the store first, never the database)."""
        if value in (False, None, "") or value == MOVED_MARKER or self.env.context.get("hams_module_secret_restore"):
            return value
        current = optional_store_secret(key)
        if current is not None and hmac.compare_digest(str(value).encode("utf-8", "replace"), current.encode("utf-8", "replace")):
            return MOVED_MARKER
        raise UserError(self.env._(
            "%(key)s is served from the secret store on this host (%(flag)s). Put the new value in the secret "
            "store first; see zero_sudo/docs/stories/secret_store_tier_a.md.",
            key=key, flag=FLAG_OPTION,
        ))

    # [@ANCHOR: zero_sudo:module_secrets_create]
    @api.model_create_multi
    def create(self, vals_list):
        keys = param_keys()
        if keys:
            vals_list = [
                dict(vals, value=self._module_secret_db_value(vals["key"], vals.get("value")))
                if vals.get("key") in keys and "value" in vals else vals
                for vals in vals_list
            ]
        return super().create(vals_list)

    # [@ANCHOR: zero_sudo:module_secrets_write]
    def write(self, vals):
        keys = param_keys()
        if "value" in vals and keys:
            secret_rows = self.filtered(lambda p: p.key in keys)
            if secret_rows:
                others = self - secret_rows
                result = True
                if others:
                    result = super(IrConfigParameterModuleSecrets, others).write(vals)
                for row in secret_rows:
                    row_vals = dict(vals, value=self._module_secret_db_value(row.key, vals["value"]))
                    result = super(IrConfigParameterModuleSecrets, row).write(row_vals) and result
                return result
        return super().write(vals)

    # [@ANCHOR: zero_sudo:module_secrets_check_store]
    @api.model
    def _check_module_secrets_in_store(self):
        """For each key served from the store whose database row still has a value (a secret or the marker):
        the store holds it. Raises SecretNotFound naming the key (never a value). A key with no row is an
        unconfigured feature and is not required."""
        keys = param_keys()
        rows = self.search([("key", "in", list(keys))], limit=len(keys))
        for row in rows:
            if row.value:
                store_secret(row.key)

    # [@ANCHOR: zero_sudo:module_secrets_register_hook]
    def _register_hook(self):
        super()._register_hook()
        self._check_module_secrets_in_store()

    # [@ANCHOR: zero_sudo:module_secrets_clear_from_database]
    @api.model
    def _hams_module_secrets_clear_from_database(self):
        """Rollout step after the flip: replace the database copies with MOVED_MARKER. Writes nothing
        unless the store holds every key that still has a database value AND holds the same value, so it
        cannot throw away the only copy of a value. Returns the keys it cleared (names only)."""
        keys = param_keys()
        if not keys:
            raise UserError(self.env._("List a group in %(flag)s before clearing the database copies.", flag=FLAG_OPTION))
        rows = self.search([("key", "in", list(keys))], limit=len(keys))
        pending = rows.filtered(lambda p: p.value and p.value != MOVED_MARKER)
        for row in pending:
            # The store must hold the SAME value: clearing the database copy of a key whose store value differs
            # (a stale store entry) would throw away the newer value.
            held = store_secret(row.key)
            if not hmac.compare_digest(row.value.encode("utf-8", "replace"), held.encode("utf-8", "replace")):
                raise UserError(self.env._(
                    "%(key)s: the database value differs from the secret store; nothing was cleared. Put the "
                    "current value in the store first.", key=row.key))
        for row in pending:
            row.write({"value": MOVED_MARKER})
        return sorted(pending.mapped("key"))

    # [@ANCHOR: zero_sudo:module_secrets_restore_to_database]
    @api.model
    def _hams_module_secrets_restore_to_database(self):
        """Rollback step before a group leaves the option: put the store's values back into the database rows
        so the host runs on the database again. Keys the store does not hold are left alone. Returns the keys
        it restored (names only)."""
        restored = []
        for key in param_keys():
            value = optional_store_secret(key)
            if value is not None:
                self.with_context(hams_module_secret_restore=True).set_param(key, value)
                restored.append(key)
        return restored
