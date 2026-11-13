# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import _, api, models
from odoo.exceptions import ValidationError

from .. import endpoint_gate as gate

# [@ANCHOR: hams_rpc_guard:parameter_parsers]
# The gate settings that are checked when they are written. Each parser raises `endpoint_gate.ConfigError` on a value
# that does not parse; the write is then refused, so a typo can never leave the endpoints open or silently closed.
PARSERS = {
    gate.ALLOWED_NETWORKS_PARAM: lambda value: gate.parse_networks(value, gate.ALLOWED_NETWORKS_PARAM,
                                                                   gate.DEFAULT_ALLOWED_NETWORKS),
    gate.TRUSTED_PROXIES_PARAM: gate.parse_trusted_proxies,
    gate.CLIENT_ADDRESS_HEADER_PARAM: gate.parse_client_address_header,
    gate.LEGACY_PARAM: gate.parse_legacy_access,
    gate.TRANSPORT_PARAM: gate.parse_transport,
    gate.UNIX_PEERS_PARAM: gate.parse_peer_rules,
}


# [@ANCHOR: hams_rpc_guard_ir_config_parameter_ir_config_parameter]
class IrConfigParameter(models.Model):
    _inherit = "ir.config_parameter"

    @api.constrains("key", "value")
    # [@ANCHOR: hams_rpc_guard:check_gate_parameters]
    def _check_hams_rpc_guard_parameters(self):
        """Refuse (ValidationError) a create or write that stores a gate setting that does not parse."""
        for record in self:
            parser = PARSERS.get(record.key)
            if parser is None:
                continue
            try:
                parser(record.value)
            except gate.ConfigError as error:
                raise ValidationError(_("Invalid RPC guard setting: %s", error)) from None

    # [@ANCHOR: hams_rpc_guard:parameter_cache_flush]
    def _flush_hams_rpc_guard_cache(self, keys):
        """Clear zero_sudo's cache of `_get_system_param` when one of `keys` is a `hams_rpc_guard.` setting.

        The gate and the method guard read every setting through `zero_sudo.security.utils._get_system_param`, a
        `@distributed_cache()` (process-lifetime L1 entry, Redis for 24 hours) that nothing clears when an
        `ir.config_parameter` is written; without this a change of `mode`, `rpc_transport`, `unix_peers` or a network list
        reaches no running worker until it is recycled. `_invalidate_model_cache` also tells every other worker."""
        if any(str(key).startswith("hams_rpc_guard.") for key in keys):
            self.env["zero_sudo.security.utils"]._invalidate_model_cache("zero_sudo.security.utils")

    @api.model_create_multi
    # [@ANCHOR: hams_rpc_guard:parameter_create_flush]
    def create(self, vals_list):
        records = super().create(vals_list)
        self._flush_hams_rpc_guard_cache(records.mapped("key"))
        return records

    # [@ANCHOR: hams_rpc_guard:parameter_write_flush]
    def write(self, vals):
        keys = self.mapped("key") + ([vals["key"]] if "key" in vals else [])
        result = super().write(vals)
        self._flush_hams_rpc_guard_cache(keys)
        return result

    # [@ANCHOR: hams_rpc_guard:parameter_unlink_flush]
    def unlink(self):
        keys = self.mapped("key")
        result = super().unlink()
        self._flush_hams_rpc_guard_cache(keys)
        return result
