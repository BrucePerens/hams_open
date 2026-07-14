# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

import os
import requests
import logging

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
            raise FileNotFoundError(
                f"Credential file {self.env_path} not found. Ensure daemon is registered and cron has run."
            )

        with open(self.env_path, "r") as f:
            for line in f:
                if line.startswith("ODOO_RPC_LOGIN="):
                    self.login = line.strip().split("=", 1)[1]
                elif line.startswith("ODOO_RPC_KEY="):
                    self.api_key = line.strip().split("=", 1)[1]

        if not self.login or not self.api_key:
            raise ValueError(
                f"Malformed credential file at {self.env_path}. Missing LOGIN or KEY."
            )
        self._authenticate()

    def _authenticate(self):
        url = f"{self.base_url}/jsonrpc"
        payload = {
            "jsonrpc": "2.0",
            "method": "call",
            "params": {
                "service": "common",
                "method": "authenticate",
                "args": [self.db_name, self.login, self.api_key, {}],
            },
            "id": 1,
        }
        response = self.session.post(url, json=payload, timeout=30)
        try:
            result = response.json()
        except ValueError as e:
            raise RuntimeError(f"Failed to decode JSON response from authenticate: {e}")
            
        self.uid = result.get("result")
        if not self.uid:
            raise RuntimeError("Authentication failed")

    def call(self, model, method, args=None, kwargs=None):
        if args is None:
            args = []
        if kwargs is None:
            kwargs = {}

        url = f"{self.base_url}/jsonrpc"
        payload = {
            "jsonrpc": "2.0",
            "method": "call",
            "params": {
                "service": "object",
                "method": "execute_kw",
                "args": [
                    self.db_name,
                    self.uid,
                    self.api_key,
                    model,
                    method,
                    args,
                    kwargs,
                ],
            },
            "id": 1,
        }

        response = self.session.post(url, json=payload, timeout=30)
        try:
            result = response.json()
        except ValueError as e:
            raise RuntimeError(f"Failed to decode JSON response: {e}")

        # Self-Healing: If key was rotated by Odoo cron, 401/403 or AccessError occurs.
        # Catch, re-read the updated .env file from disk, and retry.
        if response.status_code in (401, 403) or (
            result.get("error") and "AccessError" in str(result["error"])
        ):
            _logger.warning(
                "JSON-RPC Access Denied. Attempting to reload rotated keys from env file."
            )
            self._load_credentials()
            payload["params"]["args"][1] = self.uid
            payload["params"]["args"][2] = self.api_key

            response = self.session.post(url, json=payload, timeout=30)
            try:
                result = response.json()
            except ValueError as e:
                raise RuntimeError(f"Failed to decode JSON response on retry: {e}")

        if result.get("error"):
            raise RuntimeError(f"JSON-RPC Error: {result['error']}")

        return result.get("result")
