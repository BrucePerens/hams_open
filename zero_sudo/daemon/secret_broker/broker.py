# -*- coding: utf-8 -*-
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tier B secret broker: Odoo asks for an OPERATION on a secret; the secret never enters Odoo memory.

The broker runs as its own account (`hams_secret_broker`). Its secrets reach it through systemd
`LoadCredential=` from a root:root 0600 file, so the web account cannot read them, and the broker refuses to
start when a configured web account could (`check_not_readable`). Odoo connects to a unix socket; the
kernel says who is calling (`SO_PEERCRED`) and only the configured accounts are served.

The operation list is closed (`OPERATIONS`), and each secret names the operations it may be used for. There
is no "give me the key" operation. `https_request` sends a request to the ONE base URL configured for that
secret (the caller picks method and path inside the configured prefixes, and a few harmless headers), adds
the credential itself, follows no redirect, and removes the secret from whatever comes back.

Wire format, one request per connection, one JSON line each way:
    request   {"v": 1, "op": "<operation>", "args": {...}}
    reply     {"ok": true, "result": ...}  or  {"ok": false, "error": "<code>"}
Error codes never carry a secret or a request argument.

What this does NOT stop: a caller that IS an allowed account (a compromised Odoo process) can still ask for
every operation the policy allows, for example sign arbitrary payloads with a signing key or call the
provider API inside the configured prefixes. It cannot take the key away, use it after the process is
gone, or point it at another host. The policy (which operations, which paths) is the remaining gate.
Story: zero_sudo/docs/stories/secret_store_tier_b.md.
"""

import base64
import concurrent.futures
import grp
import http.client
import json
import logging
import os
import pwd
import socket
import ssl
import stat
import struct
import threading
import urllib.parse

if __package__:  # imported as odoo.addons.zero_sudo.daemon.secret_broker.broker
    from .. import secret_operations, secret_store
else:  # run as a daemon: main.py put zero_sudo/daemon on sys.path
    import secret_operations
    import secret_store

_logger = logging.getLogger("hams_secret_broker")

PROTOCOL_VERSION = 1
MAX_REQUEST_BYTES = 1024 * 1024
MAX_RESPONSE_BYTES = 1024 * 1024
MAX_CONCURRENT = 16
OPERATIONS = ("hmac_sign", "hmac_verify", "stripe_webhook_verify", "https_request")
CALLER_HEADERS = frozenset({"accept", "content-type", "idempotency-key", "stripe-version"})
HTTP_METHODS = frozenset({"GET", "POST", "PUT", "PATCH", "DELETE"})
AUTH_SCHEMES = ("bearer", "basic", "header")
REDACTED = "[redacted by secret broker]"

_ACL_VERSION = 2
_ACL_USER, _ACL_GROUP = 0x02, 0x08
_ACL_MASK = 0x10


class BrokerConfigError(ValueError):
    """The broker configuration is unusable; the broker refuses to start."""


class Refused(Exception):
    """A request the broker will not serve; `code` is what the caller sees."""

    # [@ANCHOR: zero_sudo:secret_broker_refused_init]
    def __init__(self, code):
        super().__init__(code)
        self.code = code


# [@ANCHOR: zero_sudo:secret_broker_validate_config]
def validate_config(config):
    """The config dict, checked: `socket` (absolute path), `allowed_users` and `deny_readers` (non-empty
    lists of account names or uids), `credentials` (`"systemd"` or an absolute file path) and `secrets`
    ({name: policy}). A policy lists `operations` (a subset of OPERATIONS) and, for https_request, an
    `https` block: `base_url` (https, no path beyond `/`, no user info), `methods`, `path_prefixes`
    (each starting with `/`), `auth` (`bearer`, `basic` with `username`, or `header` with `header_name`),
    optional `ca_file` and `timeout` (1 to 120 seconds). BrokerConfigError on anything else."""
    if not isinstance(config, dict):
        raise BrokerConfigError("the config must be a JSON object")
    sock = config.get("socket")
    if not isinstance(sock, str) or not os.path.isabs(sock):
        raise BrokerConfigError("'socket' must be an absolute path")
    for key in ("allowed_users", "deny_readers"):
        value = config.get(key)
        if not isinstance(value, list) or not value or not all(isinstance(v, (str, int)) for v in value):
            raise BrokerConfigError(f"'{key}' must be a non-empty list of account names or uids")
    credentials = config.get("credentials")
    if credentials != "systemd" and not (isinstance(credentials, str) and os.path.isabs(credentials)):
        raise BrokerConfigError("'credentials' must be 'systemd' or an absolute file path")
    secrets = config.get("secrets")
    if not isinstance(secrets, dict) or not secrets:
        raise BrokerConfigError("'secrets' must name at least one secret")
    for name, policy in secrets.items():
        secret_store.check_name(name)
        ops = policy.get("operations") if isinstance(policy, dict) else None
        if not isinstance(ops, list) or not ops or any(op not in OPERATIONS for op in ops):
            raise BrokerConfigError(f"secret {name!r}: 'operations' must be a subset of {OPERATIONS}")
        if "https_request" in ops:
            _validate_https(name, policy.get("https"))
    return config


# [@ANCHOR: zero_sudo:secret_broker_validate_https]
def _validate_https(name, https):
    """BrokerConfigError unless `https` is a complete https_request policy (see validate_config)."""
    if not isinstance(https, dict):
        raise BrokerConfigError(f"secret {name!r}: https_request needs an 'https' block")
    url = urllib.parse.urlsplit(https.get("base_url", ""))
    if url.scheme != "https" or not url.hostname or url.username or url.path not in ("", "/") or url.query:
        raise BrokerConfigError(f"secret {name!r}: 'base_url' must be https://host[:port] with nothing else")
    methods = https.get("methods")
    if not isinstance(methods, list) or not methods or not set(methods) <= HTTP_METHODS:
        raise BrokerConfigError(f"secret {name!r}: 'methods' must be a subset of {sorted(HTTP_METHODS)}")
    prefixes = https.get("path_prefixes")
    if not isinstance(prefixes, list) or not prefixes or not all(
        isinstance(p, str) and p.startswith("/") for p in prefixes
    ):
        raise BrokerConfigError(f"secret {name!r}: 'path_prefixes' must be a list of paths starting with /")
    auth = https.get("auth")
    if auth not in AUTH_SCHEMES or (auth == "basic" and not https.get("username")) or (
        auth == "header" and not https.get("header_name")
    ):
        raise BrokerConfigError(f"secret {name!r}: 'auth' must be bearer, basic (+username) or header (+header_name)")
    timeout = https.get("timeout", 20)
    if not isinstance(timeout, (int, float)) or not 1 <= timeout <= 120:
        raise BrokerConfigError(f"secret {name!r}: 'timeout' must be 1 to 120 seconds")


# [@ANCHOR: zero_sudo:secret_broker_acl_grants_read]
def acl_grants_read(raw, uid, gids):
    """True when the POSIX access ACL `raw` (the `system.posix_acl_access` xattr bytes) has a named user
    entry for `uid` or a named group entry for one of `gids` with read permission that the mask lets
    through. A malformed ACL counts as granting (fail closed)."""
    if len(raw) < 4 or (len(raw) - 4) % 8 or int.from_bytes(raw[:4], "little") != _ACL_VERSION:
        return True
    entries = []
    mask = 0o7
    for offset in range(4, len(raw), 8):
        tag = int.from_bytes(raw[offset:offset + 2], "little")
        perm = int.from_bytes(raw[offset + 2:offset + 4], "little") & 0o7
        qualifier = int.from_bytes(raw[offset + 4:offset + 8], "little")
        if tag == _ACL_MASK:
            mask = perm
        entries.append((tag, perm, qualifier))
    for tag, perm, qualifier in entries:
        named = (tag == _ACL_USER and qualifier == uid) or (tag == _ACL_GROUP and qualifier in gids)
        if named and perm & mask & 0o4:
            return True
    return False


# [@ANCHOR: zero_sudo:secret_broker_readable_by]
def readable_by(path, uid, gids, getxattr=getattr(os, "getxattr", None)):
    """True when an account with `uid` and supplementary/primary `gids` could read `path` by its owner,
    group, other bits or its POSIX ACL. Root (uid 0) can read anything. Only the file itself is judged,
    not the directories above it, so the answer errs towards "readable"."""
    if uid == 0:
        return True
    st = os.stat(path)
    if st.st_uid == uid:
        return bool(st.st_mode & stat.S_IRUSR)
    if st.st_gid in gids and st.st_mode & stat.S_IRGRP:
        return True
    if st.st_mode & stat.S_IROTH:
        return True
    if getxattr is None:
        return False
    try:
        raw = getxattr(path, "system.posix_acl_access")
    except OSError:
        return False
    return acl_grants_read(raw, uid, gids)


# [@ANCHOR: zero_sudo:secret_broker_account_ids]
def account_ids(user):
    """(uid, {gids}) of an account name or numeric uid: its primary group and every group listing it."""
    entry = pwd.getpwuid(user) if isinstance(user, int) else pwd.getpwnam(user)
    gids = {entry.pw_gid} | {g.gr_gid for g in grp.getgrall() if entry.pw_name in g.gr_mem}
    return entry.pw_uid, gids


# [@ANCHOR: zero_sudo:secret_broker_check_not_readable]
def check_not_readable(paths, deny_readers, ids=account_ids):
    """BrokerConfigError when any account in `deny_readers` (the web accounts) could read any of
    `paths`: the broker does not start on a host where the web process could read its secrets."""
    for user in deny_readers:
        uid, gids = ids(user)
        for path in paths:
            if readable_by(path, uid, gids):
                raise BrokerConfigError(f"{path} is readable by {user!r}; the web account must not read broker secrets")


# [@ANCHOR: zero_sudo:secret_broker_peer_credentials]
def peer_credentials(conn):
    """(pid, uid, gid) of the process at the other end of a connected unix socket, from the kernel."""
    raw = conn.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, struct.calcsize("3i"))
    return struct.unpack("3i", raw)


# [@ANCHOR: zero_sudo:secret_broker_redact]
def redact(text, secret, extra_forms=()):
    """`text` with every occurrence of the secret (and of each string in `extra_forms`, such as the
    base64 of a basic-auth pair) replaced by REDACTED. Secrets shorter than 4 characters are not
    searched for (they would redact ordinary text); the broker refuses to load them anyway."""
    for form in (secret, *extra_forms):
        if form and len(form) >= 4:
            text = text.replace(form, REDACTED)
    return text


# [@ANCHOR: zero_sudo:secret_broker_scrub]
def scrub(value, secret):
    """`value` (JSON-like: dict, list, str, number, bool, None) with the secret redacted from every
    string in it, keys included: the last check on every reply, whatever the operation returned."""
    if isinstance(value, str):
        return redact(value, secret)
    if isinstance(value, list):
        return [scrub(item, secret) for item in value]
    if isinstance(value, dict):
        return {scrub(k, secret): scrub(v, secret) for k, v in value.items()}
    return value


# [@ANCHOR: zero_sudo:secret_broker_check_path]
def check_path(path, prefixes):
    """Refused('path_not_allowed') unless `path` is an absolute path (query allowed) under one of
    `prefixes`, with no `..` segment, no `//`, no `@`, no `\\`, no control character and no fragment."""
    if not isinstance(path, str) or not path.startswith("/") or path.startswith("//"):
        raise Refused("path_not_allowed")
    route = path.split("?", 1)[0]
    if any(c in path for c in "@\\#") or any(ord(c) < 0x21 or ord(c) == 0x7F for c in path):
        raise Refused("path_not_allowed")
    if "//" in route or ".." in route.split("/") or "%2e" in route.lower() or "%2f" in route.lower():
        raise Refused("path_not_allowed")
    if not any(route == p.rstrip("/") or route.startswith(p.rstrip("/") + "/") for p in prefixes):
        raise Refused("path_not_allowed")
    return path


# [@ANCHOR: zero_sudo:secret_broker_https_call]
def https_call(policy, secret, method, path, body=None, headers=None):
    """Send one request to the policy's base URL with the credential added; return
    {"status", "content_type", "body"} with the secret redacted. No redirect is followed (a 3xx is returned
    as is, its Location redacted with the rest). Refused for a method or path outside the policy, a
    caller header outside CALLER_HEADERS, a non-string body, or a response above MAX_RESPONSE_BYTES."""
    if method not in policy["methods"]:
        raise Refused("method_not_allowed")
    check_path(path, policy["path_prefixes"])
    headers = dict(headers or {})
    if any(not isinstance(k, str) or k.lower() not in CALLER_HEADERS or not isinstance(v, str)
           or "\n" in v or "\r" in v for k, v in headers.items()):
        raise Refused("header_not_allowed")
    if body is not None and not isinstance(body, str):
        raise Refused("body_not_string")
    extra = []
    if policy["auth"] == "bearer":
        headers["Authorization"] = f"Bearer {secret}"
    elif policy["auth"] == "basic":
        pair = base64.b64encode(f"{policy['username']}:{secret}".encode()).decode()
        headers["Authorization"] = f"Basic {pair}"
        extra.append(pair)
    else:
        headers[policy["header_name"]] = secret
    url = urllib.parse.urlsplit(policy["base_url"])
    context = ssl.create_default_context(cafile=policy.get("ca_file"))
    conn = http.client.HTTPSConnection(url.hostname, url.port or 443, timeout=policy.get("timeout", 20),
                                       context=context)
    try:
        conn.request(method, path, body=body.encode("utf-8") if body is not None else None, headers=headers)
        response = conn.getresponse()
        data = response.read(MAX_RESPONSE_BYTES + 1)
        status, content_type = response.status, response.getheader("Content-Type", "")
        location = response.getheader("Location", "")
    finally:
        conn.close()
    if len(data) > MAX_RESPONSE_BYTES:
        raise Refused("response_too_large")
    text = data.decode("utf-8", errors="replace")
    return {
        "status": status,
        "content_type": redact(content_type, secret, extra),
        "location": redact(location, secret, extra),
        "body": redact(text, secret, extra),
    }


class Broker:
    """The request handler: policy + secret source. `handle()` is the whole decision for one request."""

    # [@ANCHOR: zero_sudo:secret_broker_init]
    def __init__(self, config, store, allowed_uids):
        self.config = validate_config(config)
        self.store = store
        self.allowed_uids = frozenset(allowed_uids)

    # [@ANCHOR: zero_sudo:secret_broker_check_secrets_present]
    def check_secrets_present(self):
        """Startup check: every configured secret is in the store and at least 8 characters long, so a
        missing or truncated credential stops the broker at start, not on the first payment."""
        for name in self.config["secrets"]:
            if len(self.store.get(name)) < 8:
                raise BrokerConfigError(f"secret {name!r} is shorter than 8 characters")

    # [@ANCHOR: zero_sudo:secret_broker_handle]
    def handle(self, raw, peer_uid):
        """The reply dict for one raw request from a peer with `peer_uid`. Refuses a peer that is not
        allowed before reading the request, then a malformed request, an unknown operation, a secret
        that is not configured or not allowed that operation, and bad arguments. The reply is checked
        once more for the secret before it leaves (`scrub`), whatever the operation returned."""
        if not isinstance(peer_uid, int) or isinstance(peer_uid, bool) or peer_uid not in self.allowed_uids:
            return {"ok": False, "error": "peer_not_allowed"}
        try:
            request = json.loads(raw.decode("utf-8"))
            if not isinstance(request, dict) or request.get("v") != PROTOCOL_VERSION:
                raise Refused("bad_request")
            op, args = request.get("op"), request.get("args")
            if op not in OPERATIONS or not isinstance(args, dict):
                raise Refused("unknown_operation")
            name = args.get("name")
            policy = self.config["secrets"].get(name) if isinstance(name, str) else None
            if policy is None:
                raise Refused("unknown_secret")
            if op not in policy["operations"]:
                raise Refused("operation_not_allowed")
            secret = self.store.get(name)
            result = self._run(op, policy, secret, args)
        except Refused as refusal:
            return {"ok": False, "error": refusal.code}
        except (ValueError, TypeError, UnicodeDecodeError):
            return {"ok": False, "error": "bad_request"}
        except (OSError, http.client.HTTPException):
            return {"ok": False, "error": "upstream_failed"}
        return {"ok": True, "result": scrub(result, secret)}

    # [@ANCHOR: zero_sudo:secret_broker_run]
    def _run(self, op, policy, secret, args):
        """Dispatch to the one implementation of `op`; unexpected arguments are bad_request."""
        allowed = {
            "hmac_sign": {"name", "payload", "digest"},
            "hmac_verify": {"name", "payload", "signature", "digest"},
            "stripe_webhook_verify": {"name", "payload", "signature_header", "tolerance", "now"},
            "https_request": {"name", "method", "path", "body", "headers"},
        }[op]
        if not set(args) <= allowed:
            raise Refused("bad_request")
        if op == "hmac_sign":
            return secret_operations.hmac_sign(secret, args.get("payload"), args.get("digest", "sha256"))
        if op == "hmac_verify":
            return secret_operations.hmac_verify(secret, args.get("payload"), args.get("signature"),
                                                 args.get("digest", "sha256"))
        if op == "stripe_webhook_verify":
            return secret_operations.stripe_webhook_verify(
                secret, args.get("payload"), args.get("signature_header"),
                args.get("tolerance", 300), args.get("now"),
            )
        return https_call(policy["https"], secret, args.get("method"), args.get("path"),
                          args.get("body"), args.get("headers"))

    # [@ANCHOR: zero_sudo:secret_broker_serve_connection]
    def serve_connection(self, conn):
        """Read one request (at most MAX_REQUEST_BYTES, 30 s), answer it, close. The log line names the
        peer uid, the operation and the outcome, never an argument or a secret."""
        with conn:
            conn.settimeout(30)
            _, peer_uid, _ = peer_credentials(conn)
            if peer_uid not in self.allowed_uids:
                reply = {"ok": False, "error": "peer_not_allowed"}
            else:
                data = b""
                while not data.endswith(b"\n") and len(data) <= MAX_REQUEST_BYTES:
                    chunk = conn.recv(65536)
                    if not chunk:
                        break
                    data += chunk
                reply = ({"ok": False, "error": "request_too_large"} if len(data) > MAX_REQUEST_BYTES
                         else self.handle(data, peer_uid))
            _logger.info("peer uid %s: %s", peer_uid, "ok" if reply.get("ok") else reply.get("error"))
            conn.sendall(json.dumps(reply).encode("utf-8") + b"\n")
            # Close cleanly: unread request bytes at close() would reset the connection and the caller
            # would see a reset instead of the refusal. Discard at most MAX_REQUEST_BYTES until its EOF.
            conn.shutdown(socket.SHUT_WR)
            discarded = 0
            while discarded <= MAX_REQUEST_BYTES:
                chunk = conn.recv(65536)
                if not chunk:
                    break
                discarded += len(chunk)

    # [@ANCHOR: zero_sudo:secret_broker_serve]
    def serve(self, stop_event=None):
        """Listen on the configured socket (mode 0660; the directory's group decides who can connect,
        SO_PEERCRED decides who is served) and answer connections on a pool of MAX_CONCURRENT threads
        until `stop_event` is set. A connection is accepted only when a worker is free, so a flood waits
        in the kernel's listen queue instead of growing threads or memory here."""
        path = self.config["socket"]
        if os.path.lexists(path):
            os.unlink(path)
        server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        old_umask = os.umask(0o117)
        try:
            server.bind(path)
        finally:
            os.umask(old_umask)
        os.chmod(path, 0o660)
        server.listen(64)
        server.settimeout(0.5)
        free = threading.BoundedSemaphore(MAX_CONCURRENT)

        def work(conn):
            try:
                self.serve_connection(conn)
            except OSError as exc:
                _logger.warning("connection ended early: %s", exc.__class__.__name__)
            finally:
                free.release()

        with concurrent.futures.ThreadPoolExecutor(MAX_CONCURRENT, thread_name_prefix="secret-broker") as pool:
            try:
                while stop_event is None or not stop_event.is_set():
                    if not free.acquire(timeout=0.5):
                        continue
                    try:
                        conn, _ = server.accept()
                    except socket.timeout:
                        free.release()
                        continue
                    pool.submit(work, conn)
            finally:
                server.close()
