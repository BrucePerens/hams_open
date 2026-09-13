# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

import os
import requests
import logging
import json

_logger = logging.getLogger(__name__)


class SecureJSONRPCClient:
    """
    Standardized JSON-2 IPC client for external daemons.
    Reads credentials strictly from local daemon_key_manager .env files.
    Implements self-healing retry logic upon key rotation.
    """

    # [@ANCHOR: zero_sudo:json_rpc_client_init]
    def __init__(self, env_path, base_url, db_name="hams"):
        self.env_path = env_path
        self.base_url = base_url.rstrip("/")
        self.db_name = db_name
        self.login = None
        self.api_key = None
        self.session = requests.Session()
        self._load_credentials()

    # [@ANCHOR: zero_sudo:json_rpc_client_load_credentials]
    def _load_credentials(self):
        if not os.path.exists(self.env_path):
            err_msg = f"""
Credential file {self.env_path} not found. 
Ensure daemon is registered and cron has run.
""".strip()
            raise FileNotFoundError(err_msg)

        with open(self.env_path, "r") as f:
            for line in f:
                if line.startswith("ODOO_RPC_LOGIN="):
                    self.login = line.strip().split("=", 1)[1]
                elif line.startswith("ODOO_RPC_KEY="):
                    self.api_key = line.strip().split("=", 1)[1]

        if not self.login or not self.api_key:
            err_msg = f"""
Malformed credential file at {self.env_path}. 
Missing LOGIN or KEY.
""".strip()
            raise ValueError(err_msg)

    def call(self, model, method, ids=(), **kwargs):
        # bug-hunt (2026-09-13): this used to send a positional `args` list
        # wrapped inside the JSON body, and authenticated with a home-grown
        # X-Auth-Signature/X-Auth-Nonce HMAC scheme instead of a real
        # Authorization header. Neither matches Odoo's actual JSON-2 wire
        # protocol: odoo.http.Json2Dispatcher.dispatch() merges the JSON
        # body's own top-level keys straight into the endpoint's keyword
        # arguments (confirmed by reading odoo/http.py directly), so a
        # caller must send `ids` plus the target method's own real keyword
        # arguments (e.g. `domain=[...]` for search()) -- there is no
        # generic "args" envelope Odoo's route ever reads. And the route
        # itself is declared `auth='bearer'` (odoo/addons/rpc/controllers/
        # json2.py), which requires a real `Authorization: Bearer <api_key>`
        # header verified against res.users.apikeys -- confirmed by reading
        # ir_http.py's own _auth_method_bearer, no server-side code anywhere
        # in this codebase ever validated the old X-Auth-Signature headers.
        # This client was therefore non-functional against the real
        # endpoint from the start; grep confirms it has zero production
        # callers today (only its own tests, which mocked out
        # requests.Session entirely and so never caught either defect).
        # Fixed to match the real protocol and the already-working
        # reference implementation in backup_management/daemon/main.py's
        # _json2_call().
        url = f"{self.base_url}/json/2/{model}/{method}"
        payload = {"ids": list(ids), **kwargs}

        def _do_request():
            headers = {
                "X-Odoo-Database": self.db_name,
                "Content-Type": "application/json",
                "Authorization": f"Bearer {self.api_key}",
            }
            return self.session.post(url, json=payload, headers=headers, timeout=30)

        response = _do_request()
        try:
            result = response.json()
        except ValueError as e:
            err_msg = f"""
Failed to decode JSON response: {e}
""".strip()
            raise RuntimeError(err_msg)

        # bug-hunt (2026-09-13): the previous version of this check also
        # OR'd in a bare substring match ("AccessError"/"AccessDenied" in
        # str(err_obj)) against the whole, unstructured error object --
        # confirmed unnecessary AND risky by reading odoo/exceptions.py and
        # Json2Dispatcher.handle_error directly: EVERY failure a credential
        # reload could ever fix already surfaces as a real HTTP 401
        # (werkzeug Unauthorized, from _auth_method_bearer on a rejected/
        # stale bearer token) or 403 (AccessDenied/AccessError, both
        # UserError subclasses with http_status = 403) -- the status-code
        # check below is already complete and structurally guaranteed, not
        # a guess. The substring match added no real coverage and could
        # misfire on an UNRELATED business error (e.g. a 422
        # ValidationError whose own message text happens to contain the
        # word "AccessError") that a credential reload could never fix --
        # triggering a needless duplicate retry (a real risk for a
        # non-idempotent call: create/write, not just search) instead of
        # surfacing the real error immediately.
        if response.status_code in (401, 403):
            # [@ANCHOR: COMM_json_rpc_self_healing_retry]
            warn_msg = """
JSON-2 Access Denied. 
Attempting to reload rotated keys from env file.
""".strip()
            _logger.warning(warn_msg)
            # # Verified by [@ANCHOR: zero_sudo:COMM_test_call_self_healing]
            self._load_credentials()
            response = _do_request()
            try:
                result = response.json()
            except ValueError as e:
                err_msg = f"""
Failed to decode JSON response on retry: {e}
""".strip()
                raise RuntimeError(err_msg)

        if isinstance(result, dict) and result.get("error"):
            raise RuntimeError(f"JSON-2 Error: {result['error']}")

        return result.get("result", result) if isinstance(result, dict) else result
