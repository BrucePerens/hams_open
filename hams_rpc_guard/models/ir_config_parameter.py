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
