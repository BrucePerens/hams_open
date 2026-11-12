# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
import logging

from werkzeug.exceptions import NotFound

from odoo import models
from odoo.http import request

from .. import endpoint_gate as gate
from .. import policy

_logger = logging.getLogger(__name__)


# [@ANCHOR: hams_rpc_guard_ir_http_ir_http]
class IrHttp(models.AbstractModel):
    _inherit = "ir.http"

    @classmethod
    def _authenticate(cls, endpoint):
        # [@ANCHOR: hams_rpc_guard:endpoint_gate]
        """Refuse Odoo's external RPC endpoints to a caller outside `hams_rpc_guard.allowed_networks`, on a transport
        the operator closed, or (on the Unix socket) whose local user is not in `hams_rpc_guard.unix_peers`.

        Runs before the core's own authentication so a refused `/json/2` request never has its bearer key checked.
        The rules and parameters are in `endpoint_gate.py`; the Unix socket in `unix_listener.py`."""
        cls._refuse_rpc_endpoint_when_closed()
        return super()._authenticate(endpoint)

    @classmethod
    # [@ANCHOR: hams_rpc_guard:refuse_endpoint_when_closed]
    def _refuse_rpc_endpoint_when_closed(cls):
        httprequest = request.httprequest
        family = gate.endpoint_family(httprequest.path)
        if family is None:
            return
        environ = httprequest.environ
        utils = request.env["zero_sudo.security.utils"]
        try:
            mode = policy.normalise_mode(utils._get_system_param(policy.MODE_PARAM, policy.DEFAULT_MODE))
            settings = gate.read_settings(utils._get_system_param)
        except Exception:  # audit-ignore-catch-all: fail closed: a parameter that cannot be read is read as closed
            _logger.exception("Could not read the RPC guard parameters; refusing the %s endpoints.", family)
            mode, settings = policy.DEFAULT_MODE, gate.GateSettings((), legacy=gate.LEGACY_NONE,
                                                                   transport=gate.TRANSPORT_CLOSED)
        if mode == policy.MODE_OFF:
            return
        peer_data = environ.get(gate.UNIX_PEER_ENVIRON_KEY)
        if peer_data is not None:
            client_class = gate.CLASS_UNIX
            allowed, reason, logins = gate.judge_unix(family, settings, gate.Peer(**peer_data))
            if allowed:
                environ[gate.UNIX_LOGINS_ENVIRON_KEY] = logins
        else:
            client = gate.resolve_client(environ, httprequest.headers, settings.trusted_proxies,
                                         settings.client_address_header)
            client_class = gate.network_class(client)
            allowed, reason = gate.judge_tcp(family, settings, client)
        served = allowed or mode == policy.MODE_REPORT
        gate.note_request(family, client_class, allowed, reason, served)
        if not served:
            raise NotFound()
