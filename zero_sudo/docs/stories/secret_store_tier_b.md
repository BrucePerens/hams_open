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

`plan(web_user, code_dir, odoo_unit=None, python="/usr/bin/python3")` `[@ANCHOR: zero_sudo:secret_broker_provision_plan]`
returns the ordered steps; `apply(steps, dry_run, run=subprocess.run, user_exists=None)`
`[@ANCHOR: zero_sudo:secret_broker_provision_apply]` carries them out and prints each (an existing account, per
`account_exists(name)` `[@ANCHOR: zero_sudo:secret_broker_provision_account_exists]`, and an existing `broker.json` or
`secrets` file are kept); `main(argv)`
`[@ANCHOR: zero_sudo:secret_broker_provision_main]` is the command line and refuses to change anything unless run as root.

| Step | What | Mode |
|---|---|---|
| `groupadd --system hams_secret_broker_clients` | the group that may reach the socket directory | |
| `useradd --system hams_secret_broker` | the broker's account (no home, `nologin`), primary group the clients group; skipped when it exists | |
| `usermod --append --groups hams_secret_broker_clients <web user>` | the web account joins the clients group, nothing else | |
| `/etc/hams_secret_broker/` | config directory | root:root 0755 |
| `broker.json` | policy, no secret (`default_config(web_user)` `[@ANCHOR: zero_sudo:secret_broker_provision_default_config]`: the web user is both the allowed caller and the denied reader; no secrets yet, so the broker refuses to start until one is configured); written only if absent | root:root 0644 |
| `secrets` | `name=value` lines; created empty only if absent, never overwritten or read by the script | root:root 0600 |
| `hams-secret-broker.service` | `unit_text(code_dir, python)` `[@ANCHOR: zero_sudo:secret_broker_provision_unit_text]`: `User=hams_secret_broker`, `LoadCredential=secrets:...`, `RuntimeDirectory=hams_secret_broker` 0750, `UMask=0117`, `NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome`, `PrivateTmp`, `PrivateDevices`, an empty capability set, `MemoryDenyWriteExecute` | root:root 0644 |
| `<odoo unit>.d/hams_secret_broker.conf` (with `--odoo-unit`) | `odoo_dropin_text()` `[@ANCHOR: zero_sudo:secret_broker_provision_odoo_dropin_text]`: `SupplementaryGroups=hams_secret_broker_clients` only. It does not set `HAMS_SECRET_STORE`: the default store (`default_spec`) then uses the broker when its socket exists and still falls back to the Tier A file or systemd credential, which Odoo core's secrets need (`hams_core_secrets_from_store`; a broker-only spec made the registry refuse to load) | root:root 0644 |

## Configuration (`broker.json`)

`validate_config(config)` `[@ANCHOR: zero_sudo:secret_broker_validate_config]` checks it and `_validate_https(name, https)`
`[@ANCHOR: zero_sudo:secret_broker_validate_https]` checks each `https` block; anything else is `BrokerConfigError` and the
broker does not start.

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

## Startup (`main.py`)

`main(argv)` `[@ANCHOR: zero_sudo:secret_broker_main]` reads `--config` (default `/etc/hams_secret_broker/broker.json`) and
exits with status 2 and one log line naming the problem (never a value) when `build(config, environ, euid, ids)`
`[@ANCHOR: zero_sudo:secret_broker_main_build]` refuses. `build` refuses: running as root; an invalid config; an unknown
account; `credentials: systemd` without `$CREDENTIALS_DIRECTORY`; a credentials file one of `deny_readers` could read
(`check_not_readable`); a credentials path that is not root-owned and private (`check_private_file`); and, through
`Broker.check_secrets_present()` `[@ANCHOR: zero_sudo:secret_broker_check_secrets_present]`, a configured secret that is
missing or shorter than 8 characters.

### The web account cannot read the credentials

`check_not_readable(paths, deny_readers, ids=account_ids)` `[@ANCHOR: zero_sudo:secret_broker_check_not_readable]` raises
`BrokerConfigError` when any account in `deny_readers` could read any path. `account_ids(user)`
`[@ANCHOR: zero_sudo:secret_broker_account_ids]` returns `(uid, {gids})` for a name or uid (primary group plus every group
listing the account). `readable_by(path, uid, gids, getxattr)` `[@ANCHOR: zero_sudo:secret_broker_readable_by]` judges the
owner, group and other bits and the POSIX access ACL (root reads anything); `acl_grants_read(raw, uid, gids)`
`[@ANCHOR: zero_sudo:secret_broker_acl_grants_read]` parses the `system.posix_acl_access` bytes (version 2, 8-byte entries:
tag, permission, qualifier) and returns True when a named user or group entry for the account has read through the mask. A
malformed ACL counts as readable (fail closed). Only the file is judged, not the directories above it, so the check errs
toward refusing.

## Serving requests

`Broker(config, store, allowed_uids)` `[@ANCHOR: zero_sudo:secret_broker_init]` validates the config and holds the secret
source (a `SecretStore`). `serve(stop_event=None)` `[@ANCHOR: zero_sudo:secret_broker_serve]` binds the socket with mode 0660
(the clients group can connect; `SO_PEERCRED` decides who is served), and answers each connection on its own thread until
`stop_event` is set. `serve_connection(conn)` `[@ANCHOR: zero_sudo:secret_broker_serve_connection]` reads one request (at most
1 MiB, 30 s), answers, discards anything left unread so the caller sees the reply rather than a reset, and logs the peer uid
and the outcome code only. `peer_credentials(conn)` `[@ANCHOR: zero_sudo:secret_broker_peer_credentials]` returns
`(pid, uid, gid)` of the peer from the kernel. At most 16 connections (`MAX_CONCURRENT`) are served at once on a fixed
thread pool; a further connection waits in the kernel's listen queue until a worker is free, so a flood cannot grow threads
or memory in the broker.

`handle(raw, peer_uid)` `[@ANCHOR: zero_sudo:secret_broker_handle]` is the whole decision for one request. In order it
refuses: a peer uid not in `allowed_uids` (before parsing anything) `peer_not_allowed`; bad JSON or another protocol version
`bad_request`; an operation outside the closed list `unknown_operation`; a secret not configured `unknown_secret`; an
operation the secret's policy does not list `operation_not_allowed`. `_run(op, policy, secret, args)`
`[@ANCHOR: zero_sudo:secret_broker_run]` refuses arguments the operation does not take (`bad_request`, so no "key" or "url"
argument can slip through) and calls the one implementation in `secret_operations.py` or `https_call`. A provider that cannot
be reached is `upstream_failed`. Every successful result goes through `scrub(value, secret)`
`[@ANCHOR: zero_sudo:secret_broker_scrub]`, which replaces the secret in every string of the result (keys included) with
`[redacted by secret broker]` via `redact(text, secret, extra_forms)` `[@ANCHOR: zero_sudo:secret_broker_redact]` (forms
shorter than 4 characters are not searched for). A refusal is `Refused(code)` `[@ANCHOR: zero_sudo:secret_broker_refused_init]`
inside the broker and `{"ok": false, "error": code}` on the wire.

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

`https_call(policy, secret, method, path, body, headers)` `[@ANCHOR: zero_sudo:secret_broker_https_call]` refuses a method
outside the policy (`method_not_allowed`), a path that `check_path` refuses, a caller header outside the four allowed or
holding a line break (`header_not_allowed`, so the caller cannot set `Authorization` or `Host`), and a non-string body
(`body_not_string`). It adds the credential, sends one request to the policy's `base_url` with certificate and host name
verification, follows no redirect (a 3xx comes back as is, with `Location` redacted), refuses a response over 1 MiB
(`response_too_large`) and redacts the secret (and, for `basic`, the base64 pair) from the content type, location and body.
`check_path(path, prefixes)` `[@ANCHOR: zero_sudo:secret_broker_check_path]` accepts only an absolute path (query allowed)
equal to or under one of the prefixes, with no `..` segment, no encoded `.` or `/`, no `//`, no `@`, `#` or `\`, and no
control character or space.

## Outstanding

- The migrations listed in [secret_store_tier_a.md](secret_store_tier_a.md#outstanding-migrations): each module moves its
  secret to the store and, where an operation fits, to the broker.
- Provisioning from `hams_shared` (`infrastructure.py`) does not install the broker yet; `provision.py` is the supported path.
- Applying Tier B to a host we run is a separate rollout with its owner's decision.

Tests: `zero_sudo/tests/test_secret_broker.py` (real `SO_PEERCRED` over a unix socket, a real local HTTPS provider that
echoes the credential back, and the startup refusals).
