# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
import logging

from werkzeug.exceptions import NotFound

from odoo import models
from odoo.http import request

from .. import endpoint_gate as gate

_logger = logging.getLogger(__name__)


class IrHttp(models.AbstractModel):
    _inherit = "ir.http"

    @classmethod
    def _authenticate(cls, endpoint):
        # [@ANCHOR: hams_rpc_guard:endpoint_gate]
        """Refuse Odoo's external RPC endpoints to a caller that is not on this machine, unless an operator opened them.

        Runs before the core's own authentication so a refused `/json/2` request never has its bearer key checked.
        The policy, its parameters and what counts as "on this machine" are in `endpoint_gate.py`."""
        cls._refuse_rpc_endpoint_when_closed()
        return super()._authenticate(endpoint)

    @classmethod
    # [@ANCHOR: hams_rpc_guard:refuse_endpoint_when_closed]
    def _refuse_rpc_endpoint_when_closed(cls):
        family = gate.endpoint_family(request.httprequest.path)
        if family is None:
            return
        param = gate.PARAM_OF_FAMILY[family]
        default = gate.DEFAULT_OF_FAMILY[family]
        utils = request.env["zero_sudo.security.utils"]
        try:
            policy = gate.normalise_policy(utils._get_system_param(param, default), default)
            trusted = gate.parse_trusted_proxies(utils._get_system_param(gate.TRUSTED_PROXIES_PARAM, ""))
        except Exception:  # audit-ignore-catch-all: fail closed: a parameter that cannot be read is read as the strict policy
            _logger.exception("Could not read the RPC guard parameters; treating the %s endpoints as loopback-only.", family)
            policy, trusted = gate.POLICY_LOOPBACK, []
        local = gate.origin_is_local(request.httprequest.environ, request.httprequest.headers, trusted)
        if not gate.policy_allows(policy, local):
            gate.note_denial(family, policy)
            raise NotFound()
