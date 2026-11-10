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

## Asking for a secret `[@ANCHOR: zero_sudo:secret_store_get]`

`SecretStore.get(name)` returns the secret from the first backend that has it and raises `SecretNotFound`, naming the
backends it tried and never a value, when none does. There is no fall back to `ir.config_parameter`, a field or the
environment. `SecretStore.require(*names)` `[@ANCHOR: zero_sudo:secret_store_require]` is the startup check a module calls
so a missing secret is a loud error at load time rather than a failure on the first payment. `SecretStore(backends, broker)`
`[@ANCHOR: zero_sudo:secret_store_init]` takes the ordered backends and an optional Tier B broker.

A name `[@ANCHOR: zero_sudo:secret_store_check_name]` is lower case letters, digits and `_.-`, at most 100 characters, with no
`/` and no `..` (it is a file name in the systemd backend).

## Asking for an operation instead

A module that only needs to sign or verify should not hold the key at all. `hmac_sign` `[@ANCHOR: zero_sudo:secret_store_hmac_sign]`,
`hmac_verify` `[@ANCHOR: zero_sudo:secret_store_hmac_verify]` and `stripe_webhook_verify`
`[@ANCHOR: zero_sudo:secret_store_stripe_webhook_verify]` go to the Tier B broker when one is configured and otherwise run
locally on the Tier A secret, using the same functions the broker uses (`secret_operations.py`): `hmac_sign`
`[@ANCHOR: zero_sudo:secret_operations_hmac_sign]` (hex HMAC, sha256 or sha512 only), `hmac_verify`
`[@ANCHOR: zero_sudo:secret_operations_hmac_verify]` (constant-time compare) and `stripe_webhook_verify`
`[@ANCHOR: zero_sudo:secret_operations_stripe_webhook_verify]` (Stripe's `t=...,v1=...` header, the `"<t>.<payload>"` HMAC and
a timestamp tolerance; a missing or zero tolerance is a failed check, never a skipped one). Module code therefore does not
change when the host moves from Tier A to Tier B. `https_request` `[@ANCHOR: zero_sudo:secret_store_https_request]` exists
only on Tier B (the broker calls the provider for you); without a broker it raises.

## The backends

**systemd credentials** (`SystemdCredentialBackend`, `[@ANCHOR: zero_sudo:secret_store_systemd_init]`,
`[@ANCHOR: zero_sudo:secret_store_systemd_get]`). One file per secret in `$CREDENTIALS_DIRECTORY`, filled by systemd from
`LoadCredential=name:/etc/hams_secret_store/name` or `LoadCredentialEncrypted=` in the Odoo unit. The source file stays
`root:root 0600`; only this service sees the copy. A file there with a world bit is refused, because systemd never writes one.

```ini
# /etc/systemd/system/odoo.service.d/secrets.conf
[Service]
LoadCredential=stripe.secret_key:/etc/hams_secret_store/stripe.secret_key
Environment=HAMS_SECRET_STORE=systemd
```

**Root-owned credentials file** (`CredentialsFileBackend`, `[@ANCHOR: zero_sudo:secret_store_file_init]`,
`[@ANCHOR: zero_sudo:secret_store_file_get]`). One file of `name=value` lines (`#` comments, blank lines allowed; parsed by
`parse_credentials_text` `[@ANCHOR: zero_sudo:secret_store_parse_credentials_text]`, whose errors name a line number, never a
value), default `/etc/hams_secret_store/secrets`, `root:odoo 0640`. `check_private_file`
`[@ANCHOR: zero_sudo:secret_store_check_private_file]` runs on every read and refuses a symlink, any world bit, a group-writable
file and an owner other than root, so the account that reads it cannot rewrite it, and a file loosened later is refused rather
than trusted. Files are read without following symlinks and capped at 64 KiB (`read_small_file`
`[@ANCHOR: zero_sudo:secret_store_read_small_file]`).

```sh
sudo install -d -o root -g odoo -m 0750 /etc/hams_secret_store
sudo install -o root -g odoo -m 0640 /dev/null /etc/hams_secret_store/secrets
sudoedit /etc/hams_secret_store/secrets      # stripe.secret_key=sk_live_...
```

**Encrypted fallback** (`EncryptedBackend`, `[@ANCHOR: zero_sudo:secret_store_encrypted_init]`,
`[@ANCHOR: zero_sudo:secret_store_encrypted_get]`), for an installer who wants the values themselves in a file that can be
copied or backed up freely. Each value is AES-256-GCM (`encrypt_value` `[@ANCHOR: zero_sudo:secret_store_encrypt_value]`,
`decrypt_value` `[@ANCHOR: zero_sudo:secret_store_decrypt_value]`) with the secret's name as associated data, so a value moved
to another name does not decrypt, and a wrong key is an error, never "not found". The 32-byte key is outside the database: a
root-owned key file (`key_from_file` `[@ANCHOR: zero_sudo:secret_store_key_from_file]`, urlsafe base64) or a systemd credential.
The values are a JSON object of tokens (`json_file_lookup` `[@ANCHOR: zero_sudo:secret_store_json_file_lookup]`). This is the
weakest backend: it is only as good as the place the key is kept, and the key is in the Odoo process like any Tier A secret.

## Choosing backends

`HAMS_SECRET_STORE` names the backends in order, for example `systemd,file:/etc/hams_secret_store/secrets` or
`broker:/run/hams_secret_broker/broker.sock` (`store_from_spec` `[@ANCHOR: zero_sudo:secret_store_store_from_spec]`; an unknown
entry, or `systemd` without `$CREDENTIALS_DIRECTORY`, is a startup error). When it is unset, `default_spec`
`[@ANCHOR: zero_sudo:secret_store_default_spec]` picks the safest the host offers: the broker if its socket exists, then systemd
credentials if the unit has any, then the root-owned file if it exists. `default_store` `[@ANCHOR: zero_sudo:secret_store_default_store]`
builds the process's store from those. With nothing configured every `get` raises: there is no plaintext default.

The Tier B client (`BrokerClient`, `[@ANCHOR: zero_sudo:secret_store_broker_client_init]`,
`[@ANCHOR: zero_sudo:secret_store_broker_client_call]`) sends one JSON line per connection to the broker's unix socket and
raises `SecretStoreError` with the broker's error code on a refusal. It sends no credential: the broker identifies the caller
from the kernel (`SO_PEERCRED`).

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

### `ir.config_parameter._check_core_secrets_in_store()` `[@ANCHOR: zero_sudo:core_secrets_check_store]` and `_register_hook()` `[@ANCHOR: zero_sudo:core_secrets_register_hook]`

Flag off: does nothing and does not read the store. Flag on: `store_value("database.secret")`, and the VAPID private key too
whenever the database has a `mail.web_push_vapid_public_key`. Raises `SecretNotFound` naming the missing key. `_register_hook`
runs it once per registry load, so a flag-on host with an incomplete store refuses to load the database (fail fast at start-up)
instead of serving pages whose CSRF check or push signature fails later.

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
