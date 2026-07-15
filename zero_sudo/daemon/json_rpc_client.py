# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

import os
import requests
import logging
import hmac
import hashlib
import time
import secrets
import json

_logger = logging.getLogger(__name__)


class SecureJSONRPCClient:
    """
    Standardized JSON-2 IPC client for external daemons.
    Reads credentials strictly from local daemon_key_manager .env files.
    Implements self-healing retry logic upon key rotation.
    """

    def __init__(self, env_path, base_url, db_name="hams"):
        self.env_path = env_path
        self.base_url = base_url.rstrip("/")
        self.db_name = db_name
        self.login = None
        self.uid = None
        self.api_key = None
        self.session = requests.Session()
        self._load_credentials()

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

    def call(self, model, method, args=None, kwargs=None):
        if args is None:
            args = []
        if kwargs is None:
            kwargs = {}

        url = f"{self.base_url}/json/2/{model}/{method}"
        payload = kwargs.copy()
        if args:
            payload["args"] = args

        def _do_request():
            timestamp = str(int(time.time()))
            nonce = secrets.token_hex(16)
            payload_str = json.dumps(payload)
            message = f"{timestamp}|{nonce}|{payload_str}".encode("utf-8")
            signature = hmac.new(self.api_key.encode("utf-8"), message, hashlib.sha256).hexdigest()

            headers = {
                "X-Odoo-Database": self.db_name,
                "Content-Type": "application/json",
                "X-Auth-User": self.login,
                "X-Auth-Timestamp": timestamp,
                "X-Auth-Nonce": nonce,
                "X-Auth-Signature": signature,
            }
            if self.uid:
                headers["X-Odoo-Service-Uid"] = str(self.uid)

            return self.session.post(url, json=payload, headers=headers, timeout=30)

        response = _do_request()
        try:
            result = response.json()
        except ValueError as e:
            err_msg = f"""
Failed to decode JSON response: {e}
""".strip()
            raise RuntimeError(err_msg)

        err_obj = result.get("error") if isinstance(result, dict) else None
        is_access_err = err_obj and "AccessError" in str(err_obj)
        if response.status_code in (401, 403) or is_access_err:
            warn_msg = """
JSON-2 Access Denied. 
Attempting to reload rotated keys from env file.
""".strip()
            _logger.warning(warn_msg)
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
