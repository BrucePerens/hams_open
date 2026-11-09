# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""A small JSON-2 client for daemons, over TCP (`http://`, `https://`) or over the Unix socket of `hams_rpc_guard`
(`unix:///run/odoo/rpc.sock`). Standard library only, so any daemon can copy or import it.

On the Unix socket the kernel tells Odoo which local user is calling (`SO_PEERCRED`), so the daemon's identity is
its Unix account as well as its API key; see `hams_rpc_guard/README.md`, "Unix socket".

    client = OdooRpcClient("unix:///run/odoo/rpc.sock", api_key, "hams_prod")
    count = client.call("ham.callbook", "search_count", domain=[])
"""
import http.client
import json
import socket
import urllib.parse

UNIX_SCHEME = "unix"
UNIX_HOST = "unix-socket"  # the Host header on the socket; Odoo serves no virtual host by it there


class OdooRpcError(Exception):
    """A JSON-2 call that did not succeed: `status` (int, the HTTP status; 0 when no answer arrived) and `body` (the
    first 500 characters of the answer, for the daemon's own log; Odoo's error answers carry no secret)."""

    def __init__(self, status, body):
        super().__init__("JSON-2 call failed with HTTP %s: %s" % (status, body))
        self.status = status
        self.body = body


# [@ANCHOR: zero_sudo:unix_http_connection]
class UnixHTTPConnection(http.client.HTTPConnection):
    """`http.client.HTTPConnection` that connects to the Unix socket `path` instead of a TCP address. The Host header
    is `unix-socket`."""

    def __init__(self, path, timeout=30, host=UNIX_HOST):
        super().__init__(host, timeout=timeout)
        self.socket_path = path

    def connect(self):
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.settimeout(self.timeout)
        try:
            sock.connect(self.socket_path)
        except OSError:
            sock.close()
            raise
        self.sock = sock


# [@ANCHOR: zero_sudo:odoo_rpc_client]
class OdooRpcClient:
    """JSON-2 calls to one Odoo database. `url` is `unix://<absolute socket path>`, `http://host[:port]` or
    `https://host[:port]`; `api_key` the service account's API key (sent as a bearer token, never logged); `database`
    the database name (header `X-Odoo-Database`); `timeout` seconds per socket operation; `host` the Host header on the
    Unix socket (default `unix-socket`; set it when the server's `dbfilter` uses `%h` or `%d`). One connection per call."""

    def __init__(self, url, api_key, database, timeout=30, host=UNIX_HOST):
        parsed = urllib.parse.urlsplit(url)
        if parsed.scheme == UNIX_SCHEME:
            if not parsed.path.startswith("/") or parsed.netloc:
                raise ValueError("A Unix socket URL is unix:///absolute/path, not %r." % url)
        elif parsed.scheme not in ("http", "https") or not parsed.hostname:
            raise ValueError("Unsupported Odoo URL %r: use unix://, http:// or https://." % url)
        self.parsed = parsed
        self.api_key = api_key
        self.database = database
        self.timeout = timeout
        self.host = host

    def _connection(self):
        parsed = self.parsed
        if parsed.scheme == UNIX_SCHEME:
            return UnixHTTPConnection(parsed.path, timeout=self.timeout, host=self.host)
        if parsed.scheme == "https":
            return http.client.HTTPSConnection(parsed.hostname, parsed.port, timeout=self.timeout)
        return http.client.HTTPConnection(parsed.hostname, parsed.port, timeout=self.timeout)

    # [@ANCHOR: zero_sudo:odoo_rpc_client_call]
    def call(self, model, method, **params):
        """POST `params` as JSON to `/json/2/<model>/<method>` and return the decoded answer. Raises `OdooRpcError`
        for any status other than 200 (404 when the RPC guard refused the caller or the route does not exist, 401 for
        a bad key, 403/422 for an access or argument error) and `OSError` when the socket cannot be reached."""
        prefix = "" if self.parsed.scheme == UNIX_SCHEME else self.parsed.path.rstrip("/")
        path = "%s/json/2/%s/%s" % (prefix, urllib.parse.quote(model), urllib.parse.quote(method))
        body = json.dumps(params).encode()
        headers = {
            "Content-Type": "application/json",
            "Authorization": "Bearer " + self.api_key,
            "X-Odoo-Database": self.database,
        }
        connection = self._connection()
        try:
            connection.request("POST", path, body=body, headers=headers)
            response = connection.getresponse()
            data = response.read()
        finally:
            connection.close()
        if response.status != 200:
            raise OdooRpcError(response.status, data[:500].decode("utf-8", "replace"))
        return json.loads(data) if data else None
