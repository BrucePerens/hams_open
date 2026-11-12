<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: Secrets Live Outside the Database (Secret Store, Tier A)

As a **Platform Operator** running these modules on my own stock Odoo 19 Community server, I want my Stripe, S3, Cloudflare,
AI-provider and mail credentials to stay out of the database, so that a database dump, an SQL-injection read, a
`ir.config_parameter` read over RPC or a stolen backup does not hand them to whoever has it. As a **Developer**, I want one
place to ask for a secret, so that I never read one from `ir.config_parameter`, a model field or `os.environ` again.

The code is `zero_sudo/daemon/secret_store.py` and `zero_sudo/daemon/secret_operations.py`. Both are plain Python with no
`odoo` import, so a standalone daemon (the Tier B broker among them) uses the same code. Tier B (an out-of-process broker,
`secret_broker/`) has its own page: [secret_store_tier_b.md](secret_store_tier_b.md).

## What Tier A protects, and what it does not

Tier A **does** keep the secret out of: database dumps and `pg_dump` backups, SQL-injection reads, reads of
`ir.config_parameter` or of a settings field over RPC (`call_kw`, `res.config.settings.get_values`), and a stolen copy of the
database or the filestore.

Tier A **does not** protect against code running inside the Odoo process. A secret that is loaded into Odoo memory can be
read by any code in that process: a controller bug that lets an attacker run Python, a malicious or compromised module, a
debugger attached as the `odoo` account. A key held by the same process that holds the ciphertext (the old
`HAMS_CRYPTO_KEY` pattern) protects nothing against that attacker either. Against that threat use Tier B, where the secret
never enters Odoo memory.

## Why the store lives in `zero_sudo` (decision for the first PR)

Every module in this repository that holds a secret already depends on `zero_sudo`, and the store has no models, views or
data: it is a library. A new addon would add a dependency edge, a manifest and an install step to every module that migrates,
for no behaviour of its own. Bruce may still move it; the import path `odoo.addons.zero_sudo.daemon.secret_store` is the only
thing a move would change.

## Asking for a secret

`SecretStore` is the only object a module talks to. It holds an ordered list of Tier A backends and an optional Tier B broker.
There is no fall back to `ir.config_parameter`, a field or the environment: a name no backend has is an error.

### `SecretStore(backends=(), broker=None)` `[@ANCHOR: zero_sudo:secret_store_init]`

| Argument | Type | Meaning |
|---|---|---|
| `backends` | iterable of backend objects | each has a `label` and `get(name)`; tried in the order given |
| `broker` | `BrokerClient` or `None` | the Tier B client; when set, the operation methods go to it |

Copies the backends into a list and keeps the broker. No I/O, no locking, nothing raised; the object holds no secret until `get`
is called, and it keeps none afterwards. Normally built by `store_from_spec` or `default_store`, not by hand outside tests.
Tests: `zero_sudo/tests/test_secret_store.py`.

### `SecretStore.get(name)` `[@ANCHOR: zero_sudo:secret_store_get]`

| Argument | Type | Meaning |
|---|---|---|
| `name` | `str` | the secret name, checked by `check_name` |

Returns the secret (`str`) from the first backend whose `get` is not `None`. Raises `ValueError` for an invalid name,
`SecretNotFound` when no backend has it (the message names the backends tried by label, or "no Tier A backend configured", and
never a value), and whatever a backend raises (`InsecureSecretSource`, `SecretStoreError`) unchanged, so a bad file stops the
lookup rather than falling through to a weaker source. Reads files on every call (no cache); no locking. This is the only Tier A
way to obtain a secret; the broker never hands one out. Tests: `test_secret_store.py`.

### `SecretStore.require(*names)` `[@ANCHOR: zero_sudo:secret_store_require]`

| Argument | Type | Meaning |
|---|---|---|
| `*names` | `str` | the secrets the calling module needs |

The start-up check: a module calls it at load time so a missing secret is a loud error before the first payment. Returns `None`
at once when a broker is configured (Tier B, nothing to read here); otherwise calls `get` for each name in order and raises the
first `SecretNotFound` (or `ValueError`, `InsecureSecretSource`) it meets. The value read is discarded. Side effect: one file read
per name per backend. Tests: `test_secret_store.py`.

### `check_name(name)` `[@ANCHOR: zero_sudo:secret_store_check_name]`

| Argument | Type | Meaning |
|---|---|---|
| `name` | `str` | a candidate secret name |

Returns `name` unchanged when it is a string of lower case letters, digits and `_.-`, starting with a letter or digit, at most 100
characters, with no `..` (so no `/` either). Raises `ValueError` otherwise (the message holds the rejected name, so never pass a
value as a name). It is the gate on every public entry point because a name is a file name in the systemd backend and a
path-traversal attempt must not get past it. Pure function. Tests: `test_secret_store.py`.

## Asking for an operation instead

A module that only needs to sign or verify should not hold the key at all. Each operation method goes to the Tier B broker when
one is configured and otherwise runs locally on the Tier A secret with the same function the broker uses, so module code does
not change when the host moves from Tier A to Tier B. The local functions are in `secret_operations.py` (stdlib only, none
returns or logs the secret).

### `SecretStore.hmac_sign(name, payload, digest="sha256")` `[@ANCHOR: zero_sudo:secret_store_hmac_sign]`

| Argument | Type | Meaning |
|---|---|---|
| `name` | `str` | the secret that is the HMAC key |
| `payload` | `str` | the text to sign (UTF-8) |
| `digest` | `str` | `sha256` or `sha512` |

Returns the hex HMAC (`str`). With a broker: one socket round trip (`BrokerClient.call("hmac_sign", ...)`), and a refusal is a
`SecretStoreError`. Without: `get(name)` then `secret_operations.hmac_sign`. Raises `ValueError` for an unsupported digest or
a non-string payload, `SecretNotFound` for a missing key. The key never leaves the broker on Tier B.

### `SecretStore.hmac_verify(name, payload, signature, digest="sha256")` `[@ANCHOR: zero_sudo:secret_store_hmac_verify]`

| Argument | Type | Meaning |
|---|---|---|
| `name` | `str` | the secret that is the HMAC key |
| `payload` | `str` | the signed text |
| `signature` | `str` | the hex signature to check |
| `digest` | `str` | `sha256` or `sha512` |

Returns `True` or `False`. A non-string signature is `False`, not an error. The compare is constant time
(`secret_operations.hmac_verify`), locally or inside the broker. Raises `SecretNotFound` when the key is missing and
`SecretStoreError` when the broker refuses. Use this, never `==`, for a signed form field or link.

### `SecretStore.stripe_webhook_verify(name, payload, signature_header, tolerance=300, now=None)` `[@ANCHOR: zero_sudo:secret_store_stripe_webhook_verify]`

| Argument | Type | Meaning |
|---|---|---|
| `name` | `str` | the webhook signing secret |
| `payload` | `str` | the raw request body as text |
| `signature_header` | `str` | the `Stripe-Signature` header |
| `tolerance` | `int` | allowed clock difference in seconds; must be positive |
| `now` | number or `None` | the clock to use (tests); `None` is the current time |

Returns `True` only for a valid, fresh signature; every malformed input is `False`. Goes to the broker when configured, else runs
`secret_operations.stripe_webhook_verify` on the Tier A secret. Raises `SecretNotFound` for a missing secret.

### `SecretStore.https_request(name, method, path, body=None, headers=None)` `[@ANCHOR: zero_sudo:secret_store_https_request]`

| Argument | Type | Meaning |
|---|---|---|
| `name` | `str` | the secret whose configured base URL the broker calls |
| `method` | `str` | HTTP method; the broker's allow list decides |
| `path` | `str` | path under the configured base URL |
| `body` | `str`, `bytes` or `None` | request body |
| `headers` | `dict` or `None` | extra request headers |

Tier B only: the broker calls the provider with the secret attached and returns `{"status", "headers", "body"}` with the secret
removed from it. Without a broker it raises `SecretStoreError`, on purpose: a Tier A module reads its key with `get` and calls the
provider itself. The caller chooses only method and path; the host is fixed by the broker. Tests: `test_secret_broker.py`.

### `secret_operations.hmac_sign(secret, payload, digest="sha256")` `[@ANCHOR: zero_sudo:secret_operations_hmac_sign]`

| Argument | Type | Meaning |
|---|---|---|
| `secret` | `str` | the HMAC key |
| `payload` | `str` | the text to sign |
| `digest` | `str` | `sha256` or `sha512`, nothing else |

Returns the hex HMAC (`str`). Raises `ValueError` for any other digest (a weak or unknown one is refused, never defaulted) or a
payload that is not a `str`. Pure, stdlib `hmac`; used by `SecretStore` locally and by the broker, so both tiers answer the same.

### `secret_operations.hmac_verify(secret, payload, signature, digest="sha256")` `[@ANCHOR: zero_sudo:secret_operations_hmac_verify]`

| Argument | Type | Meaning |
|---|---|---|
| `secret` | `str` | the HMAC key |
| `payload` | `str` | the signed text |
| `signature` | `str` | hex signature, any case |
| `digest` | `str` | `sha256` or `sha512` |

Returns `True` when the signature equals the HMAC, else `False`. A non-string signature is `False`. The compare is
`hmac.compare_digest` on bytes (a non-ASCII signature character would raise on `str`), so timing does not reveal how many leading
characters matched. Raises `ValueError` for a bad digest or payload.

### `secret_operations.stripe_webhook_verify(secret, payload, signature_header, tolerance=300, now=None)` `[@ANCHOR: zero_sudo:secret_operations_stripe_webhook_verify]`

| Argument | Type | Meaning |
|---|---|---|
| `secret` | `str` | the webhook signing secret |
| `payload` | `str` | the raw body |
| `signature_header` | `str` | `t=<unix time>,v1=<hex>[,v1=...]` |
| `tolerance` | `int` | seconds either side of `now`; must be a positive `int` |
| `now` | number or `None` | clock override for tests |

Valid when one `v1` value is the HMAC-SHA256 of `"<t>.<payload>"` and `|now - t| <= tolerance`. Returns `False`, never raises, for
a non-string input, a missing or zero or negative tolerance (a missing tolerance is a failed check, never a skipped one), a header
with no `t` or no `v1`, or a `t` that is not 1 to 12 ASCII digits. All candidates are compared in constant time.

## The backends

Each backend has a `label` (used in error messages) and `get(name)` returning the value or `None` when it does not have the name.

**systemd credentials** (`SystemdCredentialBackend`). One file per secret in `$CREDENTIALS_DIRECTORY`, filled by systemd from
`LoadCredential=name:/etc/hams_secret_store/name` or `LoadCredentialEncrypted=` in the Odoo unit. The source file stays
`root:root 0600`; only this service sees the copy.

```ini
# /etc/systemd/system/odoo.service.d/secrets.conf
[Service]
LoadCredential=stripe.secret_key:/etc/hams_secret_store/stripe.secret_key
Environment=HAMS_SECRET_STORE=systemd
```

### `SystemdCredentialBackend(directory)` `[@ANCHOR: zero_sudo:secret_store_systemd_init]`

| Argument | Type | Meaning |
|---|---|---|
| `directory` | `str` | the credentials directory, normally `$CREDENTIALS_DIRECTORY` |

Stores the directory; no I/O and nothing raised at construction. Label `systemd`. `store_from_spec` builds it and refuses the
`systemd` entry when the environment variable is unset. Tests: `test_secret_store.py`.

### `SystemdCredentialBackend.get(name)` `[@ANCHOR: zero_sudo:secret_store_systemd_get]`

| Argument | Type | Meaning |
|---|---|---|
| `name` | `str` | the secret name (validated, so it cannot leave the directory) |

Returns the file's text, stripped, or `None` when systemd delivered no credential of that name. Raises `ValueError` for an invalid
name and `InsecureSecretSource` for something that is not a regular file or has any world bit (systemd never makes such a file, so
something else wrote it), and `SecretStoreError` over 64 KiB. Uses `lstat` and a no-follow open, so a symlink is never read.

**Root-owned credentials file** (`CredentialsFileBackend`). One file of `name=value` lines (`#` comments and blank lines
allowed), default `/etc/hams_secret_store/secrets`, `root:odoo 0640`. The reading account reads it through its group and cannot
rewrite it.

```sh
sudo install -d -o root -g odoo -m 0750 /etc/hams_secret_store
sudo install -o root -g odoo -m 0640 /dev/null /etc/hams_secret_store/secrets
sudoedit /etc/hams_secret_store/secrets      # stripe.secret_key=sk_live_...
```

### `CredentialsFileBackend(path, owner_uid=0)` `[@ANCHOR: zero_sudo:secret_store_file_init]`

| Argument | Type | Meaning |
|---|---|---|
| `path` | `str` | the credentials file |
| `owner_uid` | `int` | the uid that must own it; root by default |

Stores both; no I/O at construction, so a missing file is only an error at the first `get`. Label `file`. `store_from_spec` builds
one for each `file:<path>` entry with the default owner. Tests: `test_secret_store.py`.

### `CredentialsFileBackend.get(name)` `[@ANCHOR: zero_sudo:secret_store_file_get]`

| Argument | Type | Meaning |
|---|---|---|
| `name` | `str` | the secret name |

Returns the value, or `None` when the file has no such name. Every call runs `check_private_file` first, so a file loosened or
re-owned later is refused rather than trusted, then reads and parses the whole file (no cache, so an edit is seen at once).
Raises `ValueError` (bad name), `FileNotFoundError` (no file: the caller sees a missing store, not a missing secret),
`InsecureSecretSource`, or `SecretStoreError` for a malformed or oversized file.

### `parse_credentials_text(text)` `[@ANCHOR: zero_sudo:secret_store_parse_credentials_text]`

| Argument | Type | Meaning |
|---|---|---|
| `text` | `str` | the file's content |

Returns `{name: value}` from `name=value` lines. Blank lines and lines starting with `#` are skipped; whitespace around name and
value is stripped; a value may contain `=`. A line without `=` or with an invalid name, or a duplicate name, raises
`SecretStoreError` naming the line number only, never the value (so a log line cannot leak a secret). Pure function.

### `check_private_file(path, owner_uid=0)` `[@ANCHOR: zero_sudo:secret_store_check_private_file]`

| Argument | Type | Meaning |
|---|---|---|
| `path` | `str` | the file to check |
| `owner_uid` | `int` | the required owner; root by default |

Returns the `os.lstat` result. Raises `InsecureSecretSource` for a symlink or non-regular file, any world bit (mode 007), a
group-writable file (020), or an owner other than `owner_uid`; `FileNotFoundError` when it is missing. Run on every read, so the
account that reads the file through its group cannot have rewritten it, and a file loosened later is refused. Uses `lstat`, so a
symlink is judged rather than followed.

### `read_small_file(path)` `[@ANCHOR: zero_sudo:secret_store_read_small_file]`

| Argument | Type | Meaning |
|---|---|---|
| `path` | `str` | the file to read |

Returns the bytes. Opens read-only with `O_NOFOLLOW` (a symlink fails with `OSError`) and `O_CLOEXEC`, reads at most
`MAX_SECRET_BYTES` (64 KiB) plus one byte, and raises `SecretStoreError` when the file is larger, so a runaway or hostile file
cannot fill memory. Used for every secret, key and token file in this module.

**Encrypted fallback** (`EncryptedBackend`), for an installer who wants the values themselves in a file that can be copied or
backed up freely. Each value is AES-256-GCM with the secret's name as associated data, so a value moved to another name does not
decrypt, and a wrong key is an error, never "not found". The 32-byte key is outside the database: a root-owned key file or a
systemd credential. This is the weakest backend: it is only as good as the place the key is kept, and the key is in the Odoo
process like any Tier A secret.

### `EncryptedBackend(key_loader, lookup)` `[@ANCHOR: zero_sudo:secret_store_encrypted_init]`

| Argument | Type | Meaning |
|---|---|---|
| `key_loader` | callable `() -> bytes` | returns the 32-byte key (see `key_from_file`) |
| `lookup` | callable `(name) -> str or None` | returns the stored token for a name (see `json_file_lookup`) |

Stores both callables; nothing is read until `get`, so the key is fetched fresh on each lookup and not held in the object. Label
`encrypted`. Token format: `hs1.` plus urlsafe base64 of a 12-byte nonce and the ciphertext. Tests: `test_secret_store.py`.

### `EncryptedBackend.get(name)` `[@ANCHOR: zero_sudo:secret_store_encrypted_get]`

| Argument | Type | Meaning |
|---|---|---|
| `name` | `str` | the secret name |

Returns the decrypted text, or `None` when `lookup` has no token for the name. A token that does not decrypt under this key and
name raises `SecretStoreError` rather than returning `None`, so a wrong key or a swapped value cannot look like a gap and fall
through to another backend. Also raises `ValueError` for a bad name and whatever the key loader raises.

### `encrypt_value(key, name, plaintext)` `[@ANCHOR: zero_sudo:secret_store_encrypt_value]`

| Argument | Type | Meaning |
|---|---|---|
| `key` | `bytes` | exactly 32 bytes |
| `name` | `str` | the secret name; bound in as associated data |
| `plaintext` | `str` | the secret |

Returns the token `hs1.<urlsafe base64 of nonce+ciphertext>` (`str`) from AES-256-GCM with a fresh random 12-byte nonce from
`os.urandom` and the associated data `hams-secret-store:v1:<name>`. Raises `ValueError` for a key that is not 32 bytes or an
invalid name. The token is safe to store or back up; it holds no plaintext.

### `decrypt_value(key, name, token)` `[@ANCHOR: zero_sudo:secret_store_decrypt_value]`

| Argument | Type | Meaning |
|---|---|---|
| `key` | `bytes` | the 32-byte key |
| `name` | `str` | the name the token was made for |
| `token` | `str` | a token from `encrypt_value` |

Returns the plaintext (`str`). Raises `SecretStoreError` for a token that is not `hs1` format, a key that is not 32 bytes, bad
base64, a failed GCM tag (another key, another name or a tampered token), or non-UTF-8 plaintext. The message names the secret
and never holds the token or the plaintext.

### `key_from_file(path, owner_uid=0)` `[@ANCHOR: zero_sudo:secret_store_key_from_file]`

| Argument | Type | Meaning |
|---|---|---|
| `path` | `str` | the key file: the 32-byte key as urlsafe base64 |
| `owner_uid` | `int` | the required owner; root by default |

Returns a loader function for `EncryptedBackend`. Each call of the loader runs `check_private_file` (root-owned, no world bits, not
group-writable), reads the file, decodes it and returns the 32 bytes. Raises `InsecureSecretSource` for a loose file,
`SecretStoreError` when the key does not decode to 32 bytes, `ValueError` for bad base64. Nothing is read when the loader is made.

### `json_file_lookup(path)` `[@ANCHOR: zero_sudo:secret_store_json_file_lookup]`

| Argument | Type | Meaning |
|---|---|---|
| `path` | `str` | a JSON file holding `{name: token}` |

Returns a lookup function for `EncryptedBackend`: it reads and parses the file on every call and returns the token for the name, or
`None` for a name not in it or for a missing file (an empty store). Raises `SecretStoreError` when the JSON is not an object and
`ValueError` for invalid JSON. The file holds ciphertext only, so its mode does not matter for confidentiality.

## Choosing backends

`HAMS_SECRET_STORE` names the backends in order, for example `systemd,file:/etc/hams_secret_store/secrets` or
`broker:/run/hams_secret_broker/broker.sock`. With nothing configured every `get` raises: there is no plaintext default.

### `store_from_spec(spec, environ=None)` `[@ANCHOR: zero_sudo:secret_store_store_from_spec]`

| Argument | Type | Meaning |
|---|---|---|
| `spec` | `str` | comma-separated entries: `systemd`, `file:<path>`, `encrypted:<key file>:<json file>`, `broker:<socket>` |
| `environ` | mapping or `None` | where `CREDENTIALS_DIRECTORY` is read; `None` is `os.environ` |

Returns a `SecretStore` with the backends in spec order and the last `broker:` entry as its broker. Raises `SecretStoreError` for
an unknown entry, an `encrypted` entry without exactly two paths, or `systemd` without `$CREDENTIALS_DIRECTORY`: a store that was
asked for and is not there fails at start-up. Reads no file at construction. A blank spec gives an empty store.

### `default_spec(environ=None, exists=os.path.exists)` `[@ANCHOR: zero_sudo:secret_store_default_spec]`

| Argument | Type | Meaning |
|---|---|---|
| `environ` | mapping or `None` | where `CREDENTIALS_DIRECTORY` is read |
| `exists` | callable `(path) -> bool` | the existence test (a parameter so tests need no real files) |

Returns the spec used when `$HAMS_SECRET_STORE` is unset: the safest tier the host offers, in this order: `broker:` the default
socket if it exists, `systemd` if `$CREDENTIALS_DIRECTORY` is set, then `file:` the default credentials file if it exists, joined
by commas. Returns `""` when none is present, so every `get` then raises `SecretNotFound`. Never raises.

### `default_store(environ=None)` `[@ANCHOR: zero_sudo:secret_store_default_store]`

| Argument | Type | Meaning |
|---|---|---|
| `environ` | mapping or `None` | the environment to read; `None` is `os.environ` |

Returns the process's `SecretStore`: `store_from_spec` of `$HAMS_SECRET_STORE` when that is set (an empty value means an empty
store, not the default), otherwise of `default_spec()`. Raises what `store_from_spec` raises. The variable names the store and holds
no secret. Built on every call; no module-level cache, so a changed configuration is seen at once.

### `BrokerClient(socket_path, timeout=10.0)` `[@ANCHOR: zero_sudo:secret_store_broker_client_init]`

| Argument | Type | Meaning |
|---|---|---|
| `socket_path` | `str` | the broker's unix socket |
| `timeout` | `float` | seconds for connect and each read |

The Tier B client. Stores both; no connection is made until `call`. It sends no credential: the broker identifies the caller from
the kernel (`SO_PEERCRED`) and decides from that. Tests: `test_secret_broker.py`.

### `BrokerClient.call(op, **args)` `[@ANCHOR: zero_sudo:secret_store_broker_client_call]`

| Argument | Type | Meaning |
|---|---|---|
| `op` | `str` | the broker operation (`hmac_sign`, `hmac_verify`, `stripe_webhook_verify`, `https_request`) |
| `**args` | JSON values | the operation's arguments |

Opens one unix-socket connection, sends one JSON line `{"v":1,"op":..., "args":...}`, half-closes, reads the whole reply and
closes. Returns the reply's `result`. Raises `SecretStoreError` with the broker's error code when the reply is not `ok`; socket
errors (`FileNotFoundError`, `ConnectionRefusedError`, `socket.timeout`) and `ValueError` for an unparsable reply propagate. One
connection per call, so no shared state or lock; blocking, so do not call it inside a database transaction that must stay short.


## Odoo core's secrets (`database.secret`, the Web Push VAPID private key)

Odoo core reads two secrets from `ir.config_parameter`, so both sit in every database dump: `database.secret` (CSRF tokens in
`odoo/http.py`, `odoo.tools.misc.hmac` for portal, mail and mass-mailing links, and the session token, which `res.users` reads
with its own SQL past `get_param`) and `mail.web_push_vapid_private_key` (signs every Web Push message). Core code signs with
them itself, so they are Tier A secrets: the value is in Odoo memory, and the Tier B broker cannot serve them. A store that has
only a broker does not hold them.

`zero_sudo/models/core_secrets.py` serves both from the secret store under the same names, behind a host flag. The flag is
the Odoo configuration option `hams_core_secrets_from_store` (odoo.conf, `[options]`), not a database setting, so a database
write cannot switch the source back. **Off (the default, and any host without the option): nothing changes.** The values come
from `ir.config_parameter` exactly as before and the store is never read, so a host without a store keeps working. **On:** the
store is the only source; the database row holds `moved-to-secret-store` (`MOVED_MARKER`) once the rollout clears it; every
database on the host signs with the one store value (the store is per host). Odoo logs `unknown option
'hams_core_secrets_from_store' ... stored as-is` once at start-up when the option is set; that is expected.

Rollout (copy, flip, verify, clear) and rollback (restore, unflip): hams_com `docs/runbooks/CORE_SECRETS_FROM_STORE.md`. The
store value must equal the database value at the flip, so sessions, CSRF tokens and Web Push subscriptions stay valid.

### `parse_flag(value)` `[@ANCHOR: zero_sudo:core_secrets_parse_flag]`

| Argument | Type | Meaning |
|---|---|---|
| `value` | `None`, `bool` or `str` | the option as Odoo stored it (text from odoo.conf) |

Returns `True` for `True`, `1`, `true`, `yes`, `on`; `False` for `None`, `False`, empty, `0`, `false`, `no`, `off` (any case,
surrounding blanks ignored). Anything else raises `ValueError` naming the option: a typo must not silently choose the database.
Pure function, no side effects.

### `core_secrets_from_store(config=None)` `[@ANCHOR: zero_sudo:core_secrets_from_store]`

| Argument | Type | Meaning |
|---|---|---|
| `config` | mapping with `get`, or `None` | where the option is read; `None` means Odoo's `tools.config` |

Returns `parse_flag(config.get("hams_core_secrets_from_store"))`. Raises `ValueError` for a value `parse_flag` refuses. Read
on every call (no caching), so it costs one dictionary lookup.

### `core_secret_store()` `[@ANCHOR: zero_sudo:core_secrets_store]`

No arguments. Returns the process's `SecretStore` from `default_store()` (`$HAMS_SECRET_STORE`, else the safest backends the
host offers). Raises `SecretStoreError` for a spec that names a backend that is not there. Built on every call, so a change to
the store is seen without a restart. A separate function so tests give it a store of their own.

### `store_value(key)` `[@ANCHOR: zero_sudo:core_secrets_store_value]`

| Argument | Type | Meaning |
|---|---|---|
| `key` | `str` | `database.secret` or `mail.web_push_vapid_private_key` (any valid secret name works) |

Returns the store's value (str). Raises `SecretNotFound` (names the backends tried, never a value) when no backend holds it,
`InsecureSecretSource` for a secret file with a wrong owner or mode, `SecretStoreError` for an unreadable store.

### `_optional_store_value(key)` `[@ANCHOR: zero_sudo:core_secrets_optional_store_value]`

Module-level; `key` is a core secret name. Returns the store's value (str), or `None` when no backend holds it (it catches
`SecretNotFound` only: `InsecureSecretSource` and `SecretStoreError` still propagate). The VAPID private key read and the VAPID
write comparison use it, because a missing VAPID key is an ordinary state, unlike a missing `database.secret`.

### `ir.config_parameter._get_param(key)` `[@ANCHOR: zero_sudo:core_secrets_get_param]`

The one read path under `get_param` (core's `get_param` checks read access, then calls this). Flag off, or any other key:
Odoo's own `_get_param` (ormcached database read). Flag on: `database.secret` is `store_value()` and raises `SecretNotFound`
when missing (Odoo cannot sign anything without it; core would raise "CSRF protection requires a configured database secret"
anyway); the VAPID private key is the store value or `None` when missing, which core reads as "Web Push is not set up". The
store values are not put in the ormcache. Access rules above this method (core's, and hams_com `ham_base`'s service-account
checks on `get_param`) are unchanged.

### `ir.config_parameter._core_secret_db_value(key, value)` `[@ANCHOR: zero_sudo:core_secrets_db_value]`

Called per row by `create` and `write` with the flag on; the table under the next heading is its whole contract. Returns the value
the database row gets: `value` itself for `False`, `None`, the marker, or any value while context `hams_core_secret_restore` is
set; the marker `moved-to-secret-store` for any other `database.secret` (with a log warning naming the option, never the value)
and for a VAPID private key equal to the store's (`hmac.compare_digest`); raises `UserError` (names the key and the option) for any
other VAPID private-key value.

### `ir.config_parameter.create(vals_list)` `[@ANCHOR: zero_sudo:core_secrets_create]` and `write(vals)` `[@ANCHOR: zero_sudo:core_secrets_write]`

Flag off: Odoo's own. Flag on, for a row of one of the two keys, the value written is decided per row (`_core_secret_db_value`):

| Value written | What the row gets |
|---|---|
| `False`, `None` or `moved-to-secret-store` | that value |
| anything, with context `hams_core_secret_restore` (only the restore step sets it) | that value |
| any other value for `database.secret` (Odoo writes a fresh one when it creates or duplicates a database) | `moved-to-secret-store`, with a warning in the log; the store value stays the one in use |
| the store's own value for the VAPID private key (constant-time compare) | `moved-to-secret-store` |
| any other value for the VAPID private key (a new key pair from `mail.push.device`) | `UserError`: the new private key would be lost while its public key landed in the database, and every push would fail; put the pair in the store first |

Rows of other keys in the same `write` are written unchanged. Batch-safe (`create` takes a list; `write` handles each row).

### `ir.config_parameter._check_core_secrets_in_store()` `[@ANCHOR: zero_sudo:core_secrets_check_store]`

No arguments (`@api.model` style call on `ir.config_parameter`). Flag off: returns `None` at once and does not read the store.
Flag on: `store_value("database.secret")`, and the VAPID private key too whenever the database row for
`mail.web_push_vapid_public_key` is non-empty (read with Odoo's own `_get_param`, so the check never recurses into the store
path). Returns `None`. Raises `SecretNotFound` naming the missing key, never a value; `InsecureSecretSource` or `SecretStoreError`
for a bad store. Reads store files; no database write, no locking.

### `ir.config_parameter._register_hook()` `[@ANCHOR: zero_sudo:core_secrets_register_hook]`

No arguments; Odoo calls it once per registry load. Runs core's `_register_hook`, then `_check_core_secrets_in_store()`. A flag-on
host with an incomplete store therefore refuses to load the database (fail fast at start-up) instead of serving pages whose CSRF
check or push signature fails later; a flag-off host never reads the store here. Raises what the check raises, which aborts the
registry load for that database. Tests: `zero_sudo/tests/test_core_secrets_from_store.py`.

### `ir.config_parameter._hams_core_secrets_clear_from_database()` `[@ANCHOR: zero_sudo:core_secrets_clear_from_database]`

Rollout step after the flip (Odoo shell, superuser). No arguments. Raises `UserError` when the flag is off. Checks first that
the store holds every key whose database row still has a value, raising `SecretNotFound` and writing nothing otherwise, so it
cannot throw away the only copy; then writes `moved-to-secret-store` into those rows. Returns the sorted list of keys it
cleared (names only, never values); `[]` when nothing was left to clear (idempotent).

### `ir.config_parameter._hams_core_secrets_restore_to_database()` `[@ANCHOR: zero_sudo:core_secrets_restore_to_database]`

Rollback step before the flag goes off (Odoo shell, superuser). No arguments. Writes the store's value of each key back into
its database row (through the restore context, so the flag-on guard lets it through), so the flag-off host signs with the same
secret and sessions survive. Keys the store does not hold are skipped. Returns the list of keys it restored (names only).
Works with the flag on or off; idempotent.

### `res.users._session_token_get_values()` `[@ANCHOR: zero_sudo:core_secrets_session_token_values]`

Core builds the session token key from `(column, value)` pairs it selects with SQL, including `database_secret` read straight
from `ir_config_parameter`. Flag off: core's tuple unchanged. Flag on: the `database_secret` value is replaced by
`store_value("database.secret")` (raises `SecretNotFound` when missing), so a row holding the marker never signs sessions, and a
store value equal to the old row gives the same token, which keeps every open session valid across the flip. Returns core's
`False` unchanged when the user row is missing.

## Outstanding migrations

This change adds the store; it migrates nothing yet. Every item below still holds a secret where the web process can read it,
and is migrated one module at a time, each with a migration script and a test that the value is gone from
`ir.config_parameter` and from every field. Names only (from the 2026-11-09 inventory of one production host and a code scan),
in the order that gives the largest reduction per step.

| Where it is held | Secret (name only) | Owner module | Note |
|---|---|---|---|
| process environment | `CLOUDFLARE_API_TOKEN`, `CLOUDFLARE_TUNNEL_TOKEN` | `cloudflare`, deployment | first: the account token |
| process environment | `HAMS_CRYPTO_KEY` (decrypts every `*_crypt` column) | `zero_sudo` (`_get_crypto_secret`) | it also protects backup credentials, so any change needs Bruce's decision |
| process environment | `GEMINI_API_KEY`, `SMTP_PASS`, `REDIS_PASSWORD`, `RMQ_PASS`, `DB_PASS`, `PDNS_API_KEY`, `ODOO_ADMIN_PASSWORD`, `ODOO_SERVICE_PASSWORD` | deployment (env files) | stop loading the env files into the web unit; move each to a unit credential or the broker |
| `ir.config_parameter` | `stripe.secret_key`, `stripe.webhook_secret` | Stripe dues module | webhook check becomes `stripe_webhook_verify` |
| `ir.config_parameter` | `database.secret`, `mail.web_push_vapid_private_key` | Odoo core (`base`, `mail`) | served from the store by `core_secrets.py` behind `hams_core_secrets_from_store` (above); off until the hams1 rollout flips it |
| `ir.config_parameter` | `gemini.api_key`, `form_shield.signing_key`, `parking.form_secret` | AI provider glue, `form_shield`, `parking` | the two signing keys become `hmac_sign` / `hmac_verify` |
| `ir.config_parameter` | `ham_relay_bridge.api_key`, `ham_relay_bridge.source_publish_key`, `ham_relay_bridge.turn_secret`, `ham_simulated_band.ident_token_secret` | relay bridge, simulated band | |
| settings fields | `hams_s3_aws_access_key_id`, `hams_s3_aws_secret_access_key` | `hams_s3` | |
| settings fields | `cloudflare_api_token`, `cloudflare_turnstile_secret`, `website.cloudflare_api_token_crypt` | `cloudflare` | |
| settings fields | `distributed_redis_cache.redis_password` | `distributed_redis_cache` | |
| settings fields | `ham_map_*_key`, `ham_map_opensky_password`, `ham_map_settings.*_override`, `ham_map.*_credential` | `web_map` | |
| model columns | `backup_config.secret_key_crypt` | `backup_management` | a backup credential: only Bruce changes it |
| model columns | `ham_dns_zone.acme_token`, `ham_simulated_bot_host.api_token`, `res_users.allstar_password_crypt` | DNS, simulated bots, AllStar | |
| key directory | daemon `*_service_internal.key` and daemon `.env` files owned by the web account | daemon provisioning | ownership move to root, a separate item |

Tests: `zero_sudo/tests/test_secret_store.py`, `zero_sudo/tests/test_core_secrets_from_store.py` (both flag modes).
