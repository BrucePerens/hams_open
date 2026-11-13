# -*- coding: utf-8 -*-
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Secret store: the one place a module gets a secret (Tier A) or an operation on one (Tier B).

Modules never read a secret from `ir.config_parameter`, a model field or `os.environ`. They ask this
store, which tries the backends it was configured with, in order, and raises `SecretNotFound` when none
has the name: a missing secret is a loud error for the module that needs it, never a silent fall back
to plaintext somewhere else.

Tier A backends (any stock Odoo 19 Community host, no extra daemon):

- `SystemdCredentialBackend`: one file per secret in `$CREDENTIALS_DIRECTORY`, which systemd fills from
  `LoadCredential=` / `LoadCredentialEncrypted=` in the unit. The source file can stay root:root 0600.
- `CredentialsFileBackend`: one root-owned `name=value` file (mode 0400/0440/0600/0640, no world bits,
  not writable by the group). The reading account reads it through the group; it cannot rewrite it.
- `EncryptedBackend`: AES-256-GCM values (the secret name is the associated data, so a value cannot be
  moved to another name) whose 32-byte key lives outside the database, in a systemd credential or a
  root-owned key file. The ciphertext may sit anywhere, including a backup.

What Tier A protects against, and what it does not (docs/stories/secret_store_tier_a.md): a database dump,
an SQL-injection read, a config-parameter read over RPC and a stolen backup no longer contain the secret.
It does NOT protect against code running inside the Odoo process: a secret loaded into Odoo memory can be
read by any code in that process. Tier B (`secret_broker.py`) is the answer to that: Odoo asks the broker
for an operation and the secret never enters Odoo memory. `SecretStore.hmac_sign()` and the other
operation methods use the broker when one is configured and run the same operation locally on a Tier A
secret otherwise, so module code does not change between tiers.

Plain Python, no `odoo` import (zero_sudo/daemon/README.md), so the broker and other daemons import it too.
"""

import base64
import json
import os
import re
import socket
import stat

from cryptography.exceptions import InvalidTag
from cryptography.hazmat.primitives.ciphers.aead import AESGCM

if __package__:  # imported as odoo.addons.zero_sudo.daemon.secret_store
    from . import secret_operations
else:  # a daemon that put this directory itself on sys.path (zero_sudo/daemon/README.md)
    import secret_operations

SECRET_NAME_RE = re.compile(r"^[a-z0-9][a-z0-9_.-]{0,99}$")
MAX_SECRET_BYTES = 64 * 1024
DEFAULT_CREDENTIALS_FILE = "/etc/hams_secret_store/secrets"
DEFAULT_BROKER_SOCKET = "/run/hams_secret_broker/broker.sock"
ENCRYPTED_FORMAT = b"hs1"
_AAD_PREFIX = b"hams-secret-store:v1:"


# [@ANCHOR: zero_sudo_secret_store_secret_store_error]
class SecretStoreError(RuntimeError):
    """Base class: the store could not give the caller what it asked for."""


# [@ANCHOR: zero_sudo_secret_store_secret_not_found]
class SecretNotFound(SecretStoreError):
    """No configured backend has this secret."""


# [@ANCHOR: zero_sudo_secret_store_insecure_secret_source]
class InsecureSecretSource(SecretStoreError):
    """A secret file has an owner or mode that would let the wrong account read or rewrite it."""


# [@ANCHOR: zero_sudo:secret_store_check_name]
def check_name(name):
    """The name, if it is a valid secret name (lower case, digits, `_.-`, at most 100 characters);
    otherwise ValueError. A name is also a file name in the systemd backend, so `/` and `..` never pass."""
    if not isinstance(name, str) or not SECRET_NAME_RE.match(name) or ".." in name:
        raise ValueError(f"invalid secret name {name!r}")
    return name


# [@ANCHOR: zero_sudo:secret_store_check_private_file]
def check_private_file(path, owner_uid=0):
    """Refuse (InsecureSecretSource) a secret file that is a symlink or not a regular file, has any
    world bit, is group-writable, or is not owned by `owner_uid` (root by default, so the account that
    reads the file through its group cannot rewrite it). Returns the stat result."""
    st = os.lstat(path)
    if stat.S_ISLNK(st.st_mode) or not stat.S_ISREG(st.st_mode):
        raise InsecureSecretSource(f"{path}: not a regular file")
    if st.st_mode & 0o007:
        raise InsecureSecretSource(f"{path}: mode {oct(st.st_mode & 0o777)} gives access to every account")
    if st.st_mode & 0o020:
        raise InsecureSecretSource(f"{path}: group-writable")
    if st.st_uid != owner_uid:
        raise InsecureSecretSource(f"{path}: owner uid {st.st_uid}, expected {owner_uid}")
    return st


# [@ANCHOR: zero_sudo:secret_store_read_small_file]
def read_small_file(path):
    """The bytes of `path`, opened without following a symlink, refused above MAX_SECRET_BYTES."""
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | getattr(os, "O_CLOEXEC", 0))
    with os.fdopen(fd, "rb") as handle:
        data = handle.read(MAX_SECRET_BYTES + 1)
    if len(data) > MAX_SECRET_BYTES:
        raise SecretStoreError(f"{path}: larger than {MAX_SECRET_BYTES} bytes")
    return data


# [@ANCHOR: zero_sudo:secret_store_parse_credentials_text]
def parse_credentials_text(text):
    """{name: value} from `name=value` lines; blank lines and `#` comments are skipped. Whitespace
    around the name and the value is stripped. A malformed line or a duplicate name is an error that
    names the line number only, never the value."""
    values = {}
    for number, line in enumerate(text.splitlines(), 1):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        name, sep, value = line.partition("=")
        name = name.strip()
        if not sep or not SECRET_NAME_RE.match(name):
            raise SecretStoreError(f"credentials line {number}: expected name=value")
        if name in values:
            raise SecretStoreError(f"credentials line {number}: duplicate name {name!r}")
        values[name] = value.strip()
    return values


# [@ANCHOR: zero_sudo_secret_store_systemd_credential_backend]
class SystemdCredentialBackend:
    """Tier A: `$CREDENTIALS_DIRECTORY/<name>`, one file per secret, put there by systemd."""

    label = "systemd"

    # [@ANCHOR: zero_sudo:secret_store_systemd_init]
    def __init__(self, directory):
        self.directory = directory

    # [@ANCHOR: zero_sudo:secret_store_systemd_get]
    def get(self, name):
        """The secret, or None when systemd did not deliver a credential of that name. A file with a
        world bit is refused: systemd never makes one, so something else wrote it."""
        path = os.path.join(self.directory, check_name(name))
        try:
            st = os.lstat(path)
        except FileNotFoundError:
            return None
        if not stat.S_ISREG(st.st_mode) or st.st_mode & 0o007:
            raise InsecureSecretSource(f"{path}: not a private regular file")
        return read_small_file(path).decode("utf-8").strip()


# [@ANCHOR: zero_sudo_secret_store_credentials_file_backend]
class CredentialsFileBackend:
    """Tier A: one root-owned `name=value` file, readable by the service account through its group."""

    label = "file"

    # [@ANCHOR: zero_sudo:secret_store_file_init]
    def __init__(self, path, owner_uid=0):
        self.path = path
        self.owner_uid = owner_uid

    # [@ANCHOR: zero_sudo:secret_store_file_get]
    def get(self, name):
        """The secret, or None when the file has no such name. The owner and mode are checked on every
        read (check_private_file), so a file that was loosened later is refused, not trusted."""
        check_name(name)
        check_private_file(self.path, self.owner_uid)
        return parse_credentials_text(read_small_file(self.path).decode("utf-8")).get(name)


# [@ANCHOR: zero_sudo_secret_store_encrypted_backend]
class EncryptedBackend:
    """Tier A fallback: AES-256-GCM values, key outside the database.

    `key_loader()` returns the 32-byte key (for example from a systemd credential or a root-owned key
    file, see `key_from_file`); `lookup(name)` returns the stored token or None. The token is
    `hs1.` + urlsafe base64 of nonce (12 bytes) + ciphertext, with `_AAD_PREFIX + name` as associated
    data, made by `encrypt_value`."""

    label = "encrypted"

    # [@ANCHOR: zero_sudo:secret_store_encrypted_init]
    def __init__(self, key_loader, lookup):
        self.key_loader = key_loader
        self.lookup = lookup

    # [@ANCHOR: zero_sudo:secret_store_encrypted_get]
    def get(self, name):
        """The decrypted secret, or None when `lookup` has nothing for the name. A token that does not
        decrypt under this key and name is an error, not None: a wrong key must not look like a gap."""
        token = self.lookup(check_name(name))
        if token is None:
            return None
        return decrypt_value(self.key_loader(), name, token)


# [@ANCHOR: zero_sudo:secret_store_encrypt_value]
def encrypt_value(key, name, plaintext):
    """The stored form of `plaintext` under `key` (32 bytes) for the secret `name` (str token)."""
    if len(key) != 32:
        raise ValueError("the key must be 32 bytes")
    nonce = os.urandom(12)
    sealed = AESGCM(key).encrypt(nonce, plaintext.encode("utf-8"), _AAD_PREFIX + check_name(name).encode())
    return ENCRYPTED_FORMAT.decode() + "." + base64.urlsafe_b64encode(nonce + sealed).decode()


# [@ANCHOR: zero_sudo:secret_store_decrypt_value]
def decrypt_value(key, name, token):
    """The plaintext of a token made by `encrypt_value` for the same key and name; SecretStoreError
    for a token of another format, another key or another name (the message never holds the token)."""
    prefix, _, body = token.partition(".")
    if prefix != ENCRYPTED_FORMAT.decode() or len(key) != 32:
        raise SecretStoreError(f"secret {name!r}: not a {ENCRYPTED_FORMAT.decode()} token or wrong key size")
    try:
        raw = base64.urlsafe_b64decode(body.encode())
        return AESGCM(key).decrypt(raw[:12], raw[12:], _AAD_PREFIX + check_name(name).encode()).decode("utf-8")
    except (InvalidTag, ValueError) as exc:
        raise SecretStoreError(f"secret {name!r}: does not decrypt with this key and name") from exc


# [@ANCHOR: zero_sudo:secret_store_key_from_file]
def key_from_file(path, owner_uid=0):
    """A loader for EncryptedBackend: the key file holds the 32-byte key as urlsafe base64 and passes
    check_private_file (root-owned, no world bits)."""

    def load():
        check_private_file(path, owner_uid)
        key = base64.urlsafe_b64decode(read_small_file(path).strip())
        if len(key) != 32:
            raise SecretStoreError(f"{path}: the key must decode to 32 bytes")
        return key

    return load


# [@ANCHOR: zero_sudo:secret_store_json_file_lookup]
def json_file_lookup(path):
    """A lookup for EncryptedBackend: a JSON object {name: token} in `path` (a missing file is empty).
    The file holds ciphertext only, so its mode does not matter for confidentiality."""

    def lookup(name):
        try:
            data = json.loads(read_small_file(path).decode("utf-8"))
        except FileNotFoundError:
            return None
        if not isinstance(data, dict):
            raise SecretStoreError(f"{path}: expected a JSON object")
        return data.get(name)

    return lookup


# [@ANCHOR: zero_sudo_secret_store_broker_client]
class BrokerClient:
    """Tier B client: one JSON request per connection to the broker's unix socket (secret_broker.py).
    The broker decides who may ask from SO_PEERCRED; the client sends no credential of its own."""

    # [@ANCHOR: zero_sudo:secret_store_broker_client_init]
    def __init__(self, socket_path, timeout=10.0):
        self.socket_path = socket_path
        self.timeout = timeout

    # [@ANCHOR: zero_sudo:secret_store_broker_client_call]
    def call(self, op, **args):
        """The `result` of operation `op`, or SecretStoreError with the broker's error code."""
        request = json.dumps({"v": 1, "op": op, "args": args}).encode() + b"\n"
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as conn:
            conn.settimeout(self.timeout)
            conn.connect(self.socket_path)
            conn.sendall(request)
            conn.shutdown(socket.SHUT_WR)
            chunks = []
            while True:
                chunk = conn.recv(65536)
                if not chunk:
                    break
                chunks.append(chunk)
        reply = json.loads(b"".join(chunks).decode("utf-8"))
        if reply.get("ok") is not True:
            raise SecretStoreError(f"broker refused {op}: {reply.get('error', 'unknown')}")
        return reply.get("result")


# [@ANCHOR: zero_sudo_secret_store_secret_store]
class SecretStore:
    """Ordered Tier A backends plus an optional Tier B broker.

    `get(name)` hands out a secret (Tier A only; the broker never does). The operation methods
    (`hmac_sign`, `hmac_verify`, `stripe_webhook_verify`) go to the broker when there is one and are
    otherwise computed here from `get(name)`, with the same code (secret_operations.py)."""

    # [@ANCHOR: zero_sudo:secret_store_init]
    def __init__(self, backends=(), broker=None):
        self.backends = list(backends)
        self.broker = broker

    # [@ANCHOR: zero_sudo:secret_store_get]
    def get(self, name):
        """The secret from the first backend that has it; SecretNotFound (naming the backends tried)
        when none does."""
        check_name(name)
        for backend in self.backends:
            value = backend.get(name)
            if value is not None:
                return value
        tried = ", ".join(b.label for b in self.backends) or "no Tier A backend configured"
        raise SecretNotFound(f"secret {name!r} is not in the secret store ({tried})")

    # [@ANCHOR: zero_sudo:secret_store_require]
    def require(self, *names):
        """Startup check for a module: every name is available (Tier A) or the broker is configured
        (Tier B). Raises SecretNotFound naming the first missing one."""
        if self.broker is not None:
            return
        for name in names:
            self.get(name)

    # [@ANCHOR: zero_sudo:secret_store_hmac_sign]
    def hmac_sign(self, name, payload, digest="sha256"):
        """Hex HMAC of `payload` (str) under secret `name`."""
        if self.broker is not None:
            return self.broker.call("hmac_sign", name=name, payload=payload, digest=digest)
        return secret_operations.hmac_sign(self.get(name), payload, digest)

    # [@ANCHOR: zero_sudo:secret_store_hmac_verify]
    def hmac_verify(self, name, payload, signature, digest="sha256"):
        """True when `signature` (hex) is the HMAC of `payload` under secret `name`."""
        if self.broker is not None:
            return self.broker.call("hmac_verify", name=name, payload=payload, signature=signature, digest=digest)
        return secret_operations.hmac_verify(self.get(name), payload, signature, digest)

    # [@ANCHOR: zero_sudo:secret_store_stripe_webhook_verify]
    def stripe_webhook_verify(self, name, payload, signature_header, tolerance=300, now=None):
        """True when a Stripe `Stripe-Signature` header is valid for `payload` under webhook secret
        `name` and its timestamp is within `tolerance` seconds of `now`."""
        if self.broker is not None:
            return self.broker.call(
                "stripe_webhook_verify", name=name, payload=payload,
                signature_header=signature_header, tolerance=tolerance,
            )  # the broker uses its own clock; `now` applies only to the in-process check
        return secret_operations.stripe_webhook_verify(self.get(name), payload, signature_header, tolerance, now)

    # [@ANCHOR: zero_sudo:secret_store_https_request]
    def https_request(self, name, method, path, body=None, headers=None):
        """Tier B only: the broker sends an HTTPS request to the base URL configured for secret `name`
        (the caller chooses only method and path, inside the broker's allow list) and returns
        {"status", "headers", "body"} with the secret removed. Without a broker there is no local
        equivalent on purpose: a Tier A module reads its key with get() and calls the provider itself."""
        if self.broker is None:
            raise SecretStoreError("https_request needs the Tier B broker (secret_broker.py)")
        return self.broker.call("https_request", name=name, method=method, path=path, body=body, headers=headers or {})


# [@ANCHOR: zero_sudo:secret_store_store_from_spec]
def store_from_spec(spec, environ=None):
    """A SecretStore from a spec string, comma-separated, tried in order:

    `systemd` (needs $CREDENTIALS_DIRECTORY), `file:<path>`, `encrypted:<key file>:<json file>`,
    `broker:<socket path>`. An unknown entry, or `systemd` without $CREDENTIALS_DIRECTORY, is an error:
    a store that was asked for and is not there fails at startup."""
    environ = os.environ if environ is None else environ
    backends, broker = [], None
    for entry in [e.strip() for e in spec.split(",") if e.strip()]:
        kind, _, rest = entry.partition(":")
        if kind == "systemd" and not rest:
            directory = environ.get("CREDENTIALS_DIRECTORY")
            if not directory:
                raise SecretStoreError("secret store 'systemd' needs $CREDENTIALS_DIRECTORY (LoadCredential=)")
            backends.append(SystemdCredentialBackend(directory))
        elif kind == "file" and rest:
            backends.append(CredentialsFileBackend(rest))
        elif kind == "encrypted" and rest.count(":") == 1:
            key_path, json_path = rest.split(":")
            backends.append(EncryptedBackend(key_from_file(key_path), json_file_lookup(json_path)))
        elif kind == "broker" and rest:
            broker = BrokerClient(rest)
        else:
            raise SecretStoreError(f"unknown secret store entry {entry!r}")
    return SecretStore(backends, broker)


# [@ANCHOR: zero_sudo:secret_store_default_spec]
def default_spec(environ=None, exists=os.path.exists):
    """The spec used when $HAMS_SECRET_STORE is unset: the safest tier this host offers. The broker
    when its socket exists, then systemd credentials when the unit has any, then the root-owned file
    when it exists. Empty when none is present: every get() then raises SecretNotFound."""
    environ = os.environ if environ is None else environ
    parts = []
    if exists(DEFAULT_BROKER_SOCKET):
        parts.append(f"broker:{DEFAULT_BROKER_SOCKET}")
    if environ.get("CREDENTIALS_DIRECTORY"):
        parts.append("systemd")
    if exists(DEFAULT_CREDENTIALS_FILE):
        parts.append(f"file:{DEFAULT_CREDENTIALS_FILE}")
    return ",".join(parts)


# [@ANCHOR: zero_sudo:secret_store_default_store]
def default_store(environ=None):
    """The process's store: $HAMS_SECRET_STORE when set, else default_spec()."""
    environ = os.environ if environ is None else environ
    spec = environ.get("HAMS_SECRET_STORE")  # burn-ignore-env: names the store, holds no secret
    return store_from_spec(spec if spec is not None else default_spec(environ), environ)
