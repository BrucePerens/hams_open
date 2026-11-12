<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: The Odoo Web Process Never Holds the Secret (Secret Store, Tier B)

As a **Platform Operator**, I want a bug anywhere in the roughly 66 modules running in my Odoo web process to be unable to
hand an attacker my Stripe key, my S3 secret or my Cloudflare token, because the web process never has them. As a
**Developer**, I want to call the same `SecretStore` methods as on Tier A ([secret_store_tier_a.md](secret_store_tier_a.md))
and have them work whether or not the operator runs the broker.

The code is `zero_sudo/daemon/secret_broker/` (`broker.py`, `main.py`, `provision.py`), plain Python with no `odoo` import.
It needs only systemd and Python 3 with `cryptography`; no hosted service and nothing specific to one deployment.

## What Tier B protects, and what it does not

**Protects:** the secret value. It lives in a `root:root 0600` file the broker reads through systemd `LoadCredential=`; the
broker runs as its own account; the web account can neither read the file nor the broker's memory (a different uid, and
`NoNewPrivileges`). No operation returns the secret, and every reply is scrubbed of it. A compromised web process cannot
take the key away, use it after it is gone, or send it to a host of its choosing (the bug class of `smart_download` sending
the Cloudflare Access token to arbitrary URLs).

**Does not protect against** a compromised web process using the operations the policy allows while it is compromised: it
can sign payloads with a signing key, verify webhooks, and call the provider API on the configured base URL inside the
configured methods and path prefixes. Keep each secret's policy as narrow as the module needs, and use restricted provider
keys (a Stripe restricted key, a bucket-scoped S3 key, a zone-scoped Cloudflare token) so the allowed calls are also limited
at the provider. Tier B also does not cover secrets Odoo itself must hold to run (the database password, `database.secret`
as long as Odoo core reads it directly); those stay Tier A until each has its own route.

## Installing it (any systemd host)

```sh
sudo python3 /path/to/zero_sudo/daemon/secret_broker/provision.py \
    --web-user odoo --code-dir /path/to/zero_sudo/daemon --odoo-unit odoo.service
sudoedit /etc/hams_secret_broker/secrets          # stripe.secret_key=rk_live_...
sudoedit /etc/hams_secret_broker/broker.json      # one policy per secret, see below
sudo systemctl enable --now hams-secret-broker
sudo systemctl restart odoo
```

`--dry-run` prints every step and changes nothing.

### `provision.py`

The installer is a pure planner plus an applier, so `--dry-run` shows exactly what a real run would do. Tests:
`zero_sudo/tests/test_secret_broker.py`.

| Step | What | Mode |
|---|---|---|
| `groupadd --system hams_secret_broker_clients` | the group that may reach the socket directory | |
| `useradd --system hams_secret_broker` | the broker's account (no home, `nologin`), primary group the clients group; skipped when it exists | |
| `usermod --append --groups hams_secret_broker_clients <web user>` | the web account joins the clients group, nothing else | |
| `/etc/hams_secret_broker/` | config directory | root:root 0755 |
| `broker.json` | policy, no secret (see `default_config`); written only if absent | root:root 0644 |
| `secrets` | `name=value` lines; created empty only if absent, never overwritten or read by the script | root:root 0600 |
| `hams-secret-broker.service` | the unit (see `unit_text`) | root:root 0644 |
| `<odoo unit>.d/hams_secret_broker.conf` (with `--odoo-unit`) | the drop-in (see `odoo_dropin_text`) | root:root 0644 |

#### `plan(web_user, code_dir, odoo_unit=None, python="/usr/bin/python3")` `[@ANCHOR: zero_sudo:secret_broker_provision_plan]`

| Argument | Type | Meaning |
|---|---|---|
| `web_user` | `str` | the account Odoo runs as; joins the clients group and becomes the allowed caller |
| `code_dir` | `str` | the installed `zero_sudo/daemon` directory, put in the unit's `ExecStart` |
| `odoo_unit` | `str` or `None` | the Odoo unit name (`odoo.service`); `None` writes no drop-in |
| `python` | `str` | the interpreter in `ExecStart` |

Returns the ordered list of step tuples: `("run", argv)`, `("run_if_no_user", argv)` (the `useradd`, skipped when the account
exists), `("dir", path, mode)`, `("file_if_absent", path, mode, text)` for `broker.json` and the empty `secrets` file, and
`("file", path, mode, text)` for the unit and drop-in, ending with `systemctl daemon-reload`. Pure: no I/O, no privilege needed,
and the text holds no secret. The `secrets` file comes from the plan with only a comment line and mode 0600.

#### `apply(steps, dry_run, run=subprocess.run, user_exists=account_exists)` `[@ANCHOR: zero_sudo:secret_broker_provision_apply]`

| Argument | Type | Meaning |
|---|---|---|
| `steps` | list | the output of `plan` |
| `dry_run` | `bool` | print each step and change nothing |
| `run` | callable | the command runner (tests pass a fake); called with `check=True` |
| `user_exists` | callable `(name) -> bool` | account test (tests pass a fake) |

Prints one line per step (`run:`, `dir:`, `write:`, `keep:`) and carries it out unless `dry_run`. Directories are created, then
chowned to root and chmod'ed. Files are opened `O_NOFOLLOW` with the stated mode (`O_EXCL` for `file_if_absent`, `O_TRUNC` for
`file`), written, chowned root and chmod'ed, so a planted symlink is never followed. An existing account or `file_if_absent`
target is kept untouched, so a re-run never overwrites a configured `broker.json` or the secrets. Raises
`subprocess.CalledProcessError` when a command fails (the run stops there) and `OSError` for file errors. Needs root unless dry.

#### `account_exists(name)` `[@ANCHOR: zero_sudo:secret_broker_provision_account_exists]`

| Argument | Type | Meaning |
|---|---|---|
| `name` | `str` | an account name |

Returns `True` when `pwd.getpwall()` has an entry with that name, otherwise `False`. Read-only, no side effects, no error cases
beyond what the `pwd` module raises. `apply` uses it so `useradd` is not run for an account that is already there.

#### `default_config(web_user)` `[@ANCHOR: zero_sudo:secret_broker_provision_default_config]`

| Argument | Type | Meaning |
|---|---|---|
| `web_user` | `str` | the web account |

Returns the starting `broker.json` as a dict: `socket` the default path, `allowed_users` and `deny_readers` both `[web_user]` (it
may ask, and the broker refuses to start if it could read the credentials), `credentials: "systemd"`, and `secrets: {}`. With no
secret configured `validate_config` rejects it, so the broker refuses to start until the operator adds one (fail fast). No secret
value is ever in it.

#### `unit_text(code_dir, python="/usr/bin/python3")` `[@ANCHOR: zero_sudo:secret_broker_provision_unit_text]`

| Argument | Type | Meaning |
|---|---|---|
| `code_dir` | `str` | the `zero_sudo/daemon` directory |
| `python` | `str` | the interpreter |

Returns the text of `hams-secret-broker.service`: `User=hams_secret_broker`, `Group=` the clients group, `ExecStart` running
`secret_broker/main.py --config /etc/hams_secret_broker/broker.json`, `LoadCredential=secrets:/etc/hams_secret_broker/secrets` (the
only way the secrets file reaches the broker), `RuntimeDirectory=hams_secret_broker` mode 0750, `UMask=0117`, `NoNewPrivileges`,
`ProtectSystem=strict`, `ProtectHome`, `PrivateTmp`, `PrivateDevices`, kernel and cgroup protections, address families limited to
unix and inet, `MemoryDenyWriteExecute`, an empty capability bounding set, and `Restart=on-failure`. Pure text, nothing secret.

#### `odoo_dropin_text()` `[@ANCHOR: zero_sudo:secret_broker_provision_odoo_dropin_text]`

No arguments. Returns the drop-in for the Odoo unit: `SupplementaryGroups=hams_secret_broker_clients` (so Odoo can reach the
socket directory) and nothing else. It deliberately does not set `HAMS_SECRET_STORE`: a broker-only spec would replace the default
store (`default_spec`: the broker when its socket exists, then systemd credentials, then the root-owned file), and the broker never
hands out a secret, so Odoo core's `database.secret` and VAPID key (`hams_core_secrets_from_store`) could no longer be read and the
registry would refuse to load. With the socket present `default_spec` already lists the broker first and keeps the Tier A file.
Contains no secret. Written only when `--odoo-unit` is given; Odoo must be restarted to take it.

#### `main(argv=None)` `[@ANCHOR: zero_sudo:secret_broker_provision_main]`

| Argument | Type | Meaning |
|---|---|---|
| `argv` | list or `None` | the command line: `--web-user` and `--code-dir` required, `--odoo-unit`, `--python`, `--dry-run` |

Parses the arguments, then `apply(plan(...))`. Returns `0` on success and `2` (after a message on stderr) when it is not a dry run
and the effective uid is not 0, before any change is made. `--code-dir` is made absolute. A dry run works as any user. Command
or file errors propagate from `apply`. It never reads the secrets file.

## Configuration (`broker.json`)

The broker checks the file at start-up and refuses to run on any error. `Broker.__init__` validates it again, so no `Broker`
exists with an invalid policy.

| Key | Type | Meaning |
|---|---|---|
| `socket` | absolute path | the unix socket, normally `/run/hams_secret_broker/broker.sock` |
| `allowed_users` | non-empty list of names or uids | who is served (checked by `SO_PEERCRED`) |
| `deny_readers` | non-empty list of names or uids | accounts that must not be able to read the secrets; the broker refuses to start if one can |
| `credentials` | `"systemd"` or absolute path | `systemd`: the file `secrets` in `$CREDENTIALS_DIRECTORY`; a path: a root-owned `name=value` file read directly |
| `secrets` | object `{name: policy}` | the secrets the broker serves and what each may be used for |

A policy has `operations`, a non-empty subset of `hmac_sign`, `hmac_verify`, `stripe_webhook_verify`, `https_request`, and, for
`https_request`, an `https` block:

| Key | Meaning |
|---|---|
| `base_url` | `https://host[:port]` and nothing else (no user info, path, or query); every request goes here |
| `methods` | subset of GET, POST, PUT, PATCH, DELETE |
| `path_prefixes` | paths starting with `/`; the request path must equal one or sit under it |
| `auth` | `bearer` (`Authorization: Bearer <secret>`), `basic` with `username` (`<username>:<secret>`), or `header` with `header_name` |
| `ca_file` | optional CA bundle (a private CA, or a test server); the system store otherwise |
| `timeout` | 1 to 120 seconds, default 20 |

```json
{"socket": "/run/hams_secret_broker/broker.sock", "allowed_users": ["odoo"], "deny_readers": ["odoo"],
 "credentials": "systemd",
 "secrets": {
   "stripe.secret_key": {"operations": ["https_request"], "https": {"base_url": "https://api.stripe.com",
     "methods": ["GET", "POST"], "path_prefixes": ["/v1/charges", "/v1/customers"], "auth": "bearer"}},
   "stripe.webhook_secret": {"operations": ["stripe_webhook_verify"]}}}
```

### `validate_config(config)` `[@ANCHOR: zero_sudo:secret_broker_validate_config]`

| Argument | Type | Meaning |
|---|---|---|
| `config` | `dict` | the parsed `broker.json` |

Returns `config` itself when valid. Raises `BrokerConfigError` (a `ValueError`) for: a non-object; a `socket` that is not an
absolute path; `allowed_users` or `deny_readers` empty or holding anything but names and uids; `credentials` neither `systemd` nor
an absolute path; no secrets; a secret name `check_name` rejects; `operations` empty or outside `OPERATIONS`; or an
`https_request` operation without a valid `https` block (`_validate_https`). Pure: reads no file and no secret.

### `_validate_https(name, https)` `[@ANCHOR: zero_sudo:secret_broker_validate_https]`

| Argument | Type | Meaning |
|---|---|---|
| `name` | `str` | the secret name, used only in messages |
| `https` | `dict` or other | the policy's `https` block |

Returns `None`; raises `BrokerConfigError` unless: the block is an object; `base_url` is `https` with a host and no user info,
path beyond `/`, or query; `methods` is a non-empty subset of GET, POST, PUT, PATCH, DELETE; `path_prefixes` is a non-empty list
of strings starting with `/`; `auth` is `bearer`, or `basic` with a `username`, or `header` with a `header_name`; and `timeout` is
a number from 1 to 120. A fixed `https://host[:port]` is what stops a caller from pointing the credential at another host.

## Startup (`main.py`)

The broker exits with status 2 and one log line naming the problem (never a value) when any check below fails. Tests:
`zero_sudo/tests/test_secret_broker.py` (the startup refusals).

### `build(config, environ, euid, ids=broker.account_ids)` `[@ANCHOR: zero_sudo:secret_broker_main_build]`

| Argument | Type | Meaning |
|---|---|---|
| `config` | `dict` | the loaded `broker.json` |
| `environ` | mapping | where `CREDENTIALS_DIRECTORY` is read |
| `euid` | `int` | the effective uid of the process |
| `ids` | callable | `account_ids` (a parameter for tests) |

Returns a ready `Broker`. Refuses with `BrokerConfigError`: running as root (`euid == 0`); an invalid config; `credentials:
systemd` without `$CREDENTIALS_DIRECTORY`; a credentials file one of `deny_readers` could read (`check_not_readable`). The
backend is a `CredentialsFileBackend` over `$CREDENTIALS_DIRECTORY/secrets` (owner taken from the systemd copy, because the
copy's owner depends on the systemd version) or over the configured absolute path with root as owner, so a loose path raises
`InsecureSecretSource` when read. An unknown allowed account raises `KeyError` from `pwd`. Finally `check_secrets_present`.

### `main(argv=None)` `[@ANCHOR: zero_sudo:secret_broker_main]`

| Argument | Type | Meaning |
|---|---|---|
| `argv` | list or `None` | `--config` (default `/etc/hams_secret_broker/broker.json`) |

Reads the config, calls `build` with the real environment and euid, and then `serve()` until the process is stopped (it does not
return while serving). Returns `2` after logging `secret broker refuses to start: <reason>` when reading the file, parsing it or
`build` fails with `OSError`, `ValueError` or `SecretStoreError`; returns `0` only if `serve` returns. Logs at INFO to stderr,
which systemd collects; no value or request argument is ever logged.

### The web account cannot read the credentials

The broker refuses to start on a host where the web process could read its secrets: the file is judged by owner, group, other
bits and the POSIX access ACL for each account in `deny_readers`.

#### `check_not_readable(paths, deny_readers, ids=account_ids)` `[@ANCHOR: zero_sudo:secret_broker_check_not_readable]`

| Argument | Type | Meaning |
|---|---|---|
| `paths` | list of `str` | the credential files |
| `deny_readers` | list | accounts (names or uids) that must not read them |
| `ids` | callable | `account_ids` (a parameter for tests) |

Returns `None`; raises `BrokerConfigError` naming the path and account when `readable_by` is true for any pair. `os.stat` errors
(a missing file) propagate. Read-only; run once at start-up, so a permission loosened later is not noticed until restart
(`CredentialsFileBackend` re-checks owner and mode on each read, which covers the file itself).

#### `account_ids(user)` `[@ANCHOR: zero_sudo:secret_broker_account_ids]`

| Argument | Type | Meaning |
|---|---|---|
| `user` | `str` or `int` | an account name or a numeric uid |

Returns `(uid, {gids})`: the account's primary group plus every group whose member list names it. Raises `KeyError` for an
unknown account. Read-only (`pwd` and `grp`); the result is not cached.

#### `readable_by(path, uid, gids, getxattr=os.getxattr)` `[@ANCHOR: zero_sudo:secret_broker_readable_by]`

| Argument | Type | Meaning |
|---|---|---|
| `path` | `str` | the file to judge |
| `uid` | `int` | the account's uid |
| `gids` | set of `int` | its groups |
| `getxattr` | callable or `None` | reads the ACL attribute; `None` skips ACLs (non-Linux) |

Returns `True` when the account could read the file: uid 0 always; the owner when the owner read bit is set; a group member when
the group read bit is set; anyone when the other read bit is set; or a named ACL entry (`acl_grants_read`). An unreadable ACL
attribute (`OSError`) counts as no ACL. Only the file is judged, not its directories, so it errs towards "readable" and the
broker refuses to start. `os.stat` errors propagate.

#### `acl_grants_read(raw, uid, gids)` `[@ANCHOR: zero_sudo:secret_broker_acl_grants_read]`

| Argument | Type | Meaning |
|---|---|---|
| `raw` | `bytes` | the `system.posix_acl_access` attribute |
| `uid` | `int` | the account's uid |
| `gids` | set of `int` | its groups |

Parses the ACL (a 4-byte version that must be 2, then 8-byte entries of little-endian tag, permission and qualifier) and returns
`True` when a named user entry for `uid`, or a named group entry for one of `gids`, has read permission that the mask entry lets
through. Returns `False` when no such entry exists. A malformed ACL (wrong length or version) returns `True`: it fails closed so
the broker refuses to start rather than trust something it cannot parse. Pure function.

## Serving requests

The broker answers one JSON line per connection on a unix socket. Which account is calling comes from the kernel, not from the
request.

### `Broker(config, store, allowed_uids)` `[@ANCHOR: zero_sudo:secret_broker_init]`

| Argument | Type | Meaning |
|---|---|---|
| `config` | `dict` | the broker configuration |
| `store` | `SecretStore` | where the secrets come from (Tier A backends only) |
| `allowed_uids` | iterable of `int` | the uids that are served |

Validates the config with `validate_config` (raises `BrokerConfigError`), keeps the store and a frozen set of uids. No socket is
opened and no secret read here. The instance holds no per-request state, so connections are served concurrently without a lock
around it. See `check_secrets_present`, `handle` and `serve`.

### `Broker.check_secrets_present()` `[@ANCHOR: zero_sudo:secret_broker_check_secrets_present]`

No arguments. For every secret named in the policy, reads it from the store and requires at least 8 characters, so a missing or
truncated credential stops the broker at start, not on the first payment. Returns `None`; raises `SecretNotFound` (a
`SecretStoreError`) for a missing secret, `BrokerConfigError` naming the secret for a short one (never its value), and
`InsecureSecretSource` for a loosened file. Read-only.

### `Broker.serve(stop_event=None)` `[@ANCHOR: zero_sudo:secret_broker_serve]`

| Argument | Type | Meaning |
|---|---|---|
| `stop_event` | `threading.Event` or `None` | when set, the loop ends and the socket is closed; `None` serves forever |

Removes a stale socket file, binds the configured unix socket under umask 0117, sets mode 0660 (the directory's group decides who
can connect; `SO_PEERCRED` decides who is served), listens with a backlog of 64 and accepts only when one of the
`MAX_CONCURRENT` (16) workers is free, on a thread pool. A flood therefore waits in the kernel's listen queue, and threads and
memory stay bounded. A worker's `OSError` is logged by class name only. The socket is closed on exit. Does not return unless
`stop_event` is set; raises `OSError` when the socket cannot be bound.

### `Broker.serve_connection(conn)` `[@ANCHOR: zero_sudo:secret_broker_serve_connection]`

| Argument | Type | Meaning |
|---|---|---|
| `conn` | connected unix socket | one accepted connection |

Closes the connection when done. Sets a 30 s timeout, takes the peer uid from the kernel, refuses an unallowed peer without
reading, otherwise reads up to 1 MiB until a newline (`request_too_large` beyond it), calls `handle`, logs the peer uid and the
outcome code only (never an argument or a secret), sends the reply line, and then half-closes and discards up to 1 MiB of unread
request so the caller sees the reply instead of a connection reset. Raises `OSError` on socket failure or timeout; `serve`
catches and logs it.

### `peer_credentials(conn)` `[@ANCHOR: zero_sudo:secret_broker_peer_credentials]`

| Argument | Type | Meaning |
|---|---|---|
| `conn` | connected unix socket | the connection to ask about |

Returns `(pid, uid, gid)` of the process at the other end, from `SO_PEERCRED`. The kernel fills it at connect time and the peer
cannot forge it, which is why no password or token is sent. Linux only; `OSError` on failure.

### `Broker.handle(raw, peer_uid)` `[@ANCHOR: zero_sudo:secret_broker_handle]`

| Argument | Type | Meaning |
|---|---|---|
| `raw` | `bytes` | one request line |
| `peer_uid` | `int` | the kernel-reported uid of the caller |

Returns the reply dict, never raising for a bad request. In order it refuses: a peer uid that is not an `int` in `allowed_uids`
(before parsing anything), `peer_not_allowed`; bad JSON, a non-object or another protocol version, `bad_request`; an operation
outside the closed list or non-object `args`, `unknown_operation`; a secret not configured, `unknown_secret`; an operation the
secret's policy does not list, `operation_not_allowed`. A missing or loosened secret is `secret_unavailable`, a network failure
`upstream_failed`, and invalid arguments or deeply nested JSON `bad_request`. Success is `{"ok": true, "result": ...}`, passed
through `scrub`. Reads the secret from the store on each request.

### `Broker._run(op, policy, secret, args)` `[@ANCHOR: zero_sudo:secret_broker_run]`

| Argument | Type | Meaning |
|---|---|---|
| `op` | `str` | an operation already checked against the closed list |
| `policy` | `dict` | the secret's policy |
| `secret` | `str` | the secret value, held only for this call |
| `args` | `dict` | the request's arguments |

Returns the operation's result. Raises `Refused("bad_request")` when `args` has a key the operation does not take (so no `key`
or `url` argument can slip through). Dispatches to the one implementation: `secret_operations.hmac_sign`, `hmac_verify`,
`stripe_webhook_verify` (with defaults `sha256` and tolerance 300), or `https_call`. A `ValueError` from the operation becomes
`bad_request` in `handle`.

### `scrub(value, secret)` `[@ANCHOR: zero_sudo:secret_broker_scrub]`

| Argument | Type | Meaning |
|---|---|---|
| `value` | JSON-like | the result about to leave the broker |
| `secret` | `str` | the secret to remove |

Returns a copy with the secret replaced by `[redacted by secret broker]` in every string, in lists and in dict values and keys.
Numbers, booleans and `None` pass unchanged. It is the last check on every successful reply, whatever the operation returned, so
even a provider that echoes the credential back (as the test provider does) gives nothing away. Pure function.

### `redact(text, secret, extra_forms=())` `[@ANCHOR: zero_sudo:secret_broker_redact]`

| Argument | Type | Meaning |
|---|---|---|
| `text` | `str` | text from a provider or an operation |
| `secret` | `str` | the secret |
| `extra_forms` | iterable of `str` | other encodings of it, such as the base64 of a basic-auth pair |

Returns `text` with every occurrence of the secret and of each extra form replaced by the redaction marker. A form shorter than 4
characters, or empty, is not searched for, because it would redact ordinary text (the broker refuses to load secrets under 8
characters anyway). Pure function; only the exact forms given are caught.

### `Refused(code)` `[@ANCHOR: zero_sudo:secret_broker_refused_init]`

| Argument | Type | Meaning |
|---|---|---|
| `code` | `str` | the error code the caller will see |

An exception the broker raises for a request it will not serve; `.code` is the wire error, for example `unknown_secret` or
`path_not_allowed`. Inside the broker only: `handle` turns it into `{"ok": false, "error": code}`. The code is a fixed word and
never carries a secret or a request argument.

Wire format, one JSON line each way per connection:

| Direction | Form |
|---|---|
| request | `{"v": 1, "op": "<operation>", "args": {"name": "<secret>", ...}}` |
| success | `{"ok": true, "result": <value>}` |
| refusal | `{"ok": false, "error": "<code>"}` |

| Operation | Arguments | Result |
|---|---|---|
| `hmac_sign` | `name`, `payload` (str), `digest` (`sha256` default, or `sha512`) | hex string |
| `hmac_verify` | `name`, `payload`, `signature` (hex), `digest` | bool |
| `stripe_webhook_verify` | `name`, `payload`, `signature_header`, `tolerance` (s, default 300), `now` (unix time or null) | bool |
| `https_request` | `name`, `method`, `path`, `body` (str or null), `headers` (only `Accept`, `Content-Type`, `Idempotency-Key`, `Stripe-Version`) | `{"status", "content_type", "location", "body"}`, secret redacted |

### `https_request`

#### `https_call(policy, secret, method, path, body=None, headers=None)` `[@ANCHOR: zero_sudo:secret_broker_https_call]`

| Argument | Type | Meaning |
|---|---|---|
| `policy` | `dict` | the secret's `https` block |
| `secret` | `str` | the credential to add |
| `method` | `str` | must be in the policy's `methods` |
| `path` | `str` | must pass `check_path` against `path_prefixes` |
| `body` | `str` or `None` | request body, sent as UTF-8 |
| `headers` | `dict` or `None` | caller headers: only `Accept`, `Content-Type`, `Idempotency-Key`, `Stripe-Version` |

Returns `{"status", "content_type", "location", "body"}`. Refuses with `Refused`: `method_not_allowed`, `path_not_allowed`,
`header_not_allowed` (a header outside the four, a non-string value or a line break, so the caller cannot set `Authorization` or
`Host`), `body_not_string`, `response_too_large` (over 1 MiB). It adds the credential (`bearer`, `basic` or a named header),
sends one HTTPS request to the policy's host with certificate and host-name verification (the policy's `ca_file` or the system
store) and its timeout, follows no redirect (a 3xx comes back as is), and redacts the secret and the basic-auth pair from all
returned strings. Network errors (`OSError`, `HTTPException`) propagate and `handle` reports `upstream_failed`. Blocking.

#### `check_path(path, prefixes)` `[@ANCHOR: zero_sudo:secret_broker_check_path]`

| Argument | Type | Meaning |
|---|---|---|
| `path` | `str` | the requested path, query allowed |
| `prefixes` | list of `str` | the policy's `path_prefixes` |

Returns `path` when it is an absolute path equal to, or below, one of the prefixes. Raises `Refused("path_not_allowed")` for a
non-string, a path not starting with `/` or starting with `//`, any `@`, `#` or `\`, any control character or space, a `//` or a
`..` segment in the route, an encoded `.` or `/` (`%2e`, `%2f`), or a route outside every prefix. Only the route is matched
against prefixes; the query is allowed through. Pure function, and the gate that keeps a caller inside the configured API area.

## Outstanding

- The migrations listed in [secret_store_tier_a.md](secret_store_tier_a.md#outstanding-migrations): each module moves its
  secret to the store and, where an operation fits, to the broker.
- Provisioning from `hams_shared` (`infrastructure.py`) does not install the broker yet; `provision.py` is the supported path.
- Applying Tier B to a host we run is a separate rollout with its owner's decision.

Tests: `zero_sudo/tests/test_secret_broker.py` (real `SO_PEERCRED` over a unix socket, a real local HTTPS provider that
echoes the credential back, and the startup refusals).
