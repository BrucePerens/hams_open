# HAMS Secret Canary (`hams_secret_canary`)

*Copyright © Bruce Perens K6BP.*
SPDX-License-Identifier: AGPL-3.0-or-later

The Odoo web process can read every secret it uses, so one bug anywhere can expose them. This module notices when that
happens: it plants decoy credentials where an attacker reads first, lets code register the fingerprint of a real
secret, and raises an alarm when a decoy or a registered secret appears in an HTTP response body or a log line.

**It detects a leak after it happens. It does not prevent one.** Preventing is the job of secret custody, the RPC
guard (`hams_rpc_guard`) and scoped credentials. It needs only Odoo core, `mail` and `zero_sudo`, and no outside
service: alerts go to the Odoo log, a `zero_sudo.security.log` row and an email to the administrator.

## What it does

* On install it generates a per-install salt and three decoys (never fixed strings in the repository): a Stripe-shaped
  key, an S3-shaped secret and a Cloudflare-shaped token, stored as the system parameters `stripe_secret_key_old`,
  `s3_secret_access_key_old` and `cloudflare_api_token_old`. A parameter of that name that already exists is left alone.
* Only salted SHA-256 hashes of canaries and registered secrets are stored (the key-value store key
  `hams_secret_canary.state`). The module never stores or logs a secret value, and no alarm contains one.
* `ir.http._post_dispatch` scans every finished, non-streamed text response (`text/*`, JSON, JavaScript, XML), the
  first 1 MiB of it. A log record factory scans every log record as it is created; a hit is rewritten with the value
  replaced by `[REDACTED:<label>]`.
* What is found: a registered value that appears as one token (a run of `A-Za-z0-9_-+/.`, 16 to 512 long) delimited by
  anything else (quotes, spaces, `=`, `&`, `:`, `,`, `<`). A secret that is split, encoded, or embedded inside a longer
  run of those characters is not found.
* Cost: one hash per distinct token, at most about 1 MiB of text per response (tested: under one second for 1 MiB).
* Not done: a WSGI-level scan (routes that bypass `ir.http`, such as `/xmlrpc/2/*` responses, are not scanned), a
  decoy file in `keys/` and a decoy environment variable (an install can add them and register their values with
  `_register_secret`), and wiring the real secrets of other modules to `_register_secret`.

## Alerts

Every backend in `alerts.BACKENDS` runs for every alarm; add one with `alerts.register_backend(name)`. Shipped: `logging`
(one CRITICAL line on the logger `hams_secret_canary.alarm`), `record` (a `zero_sudo.security.log` row, reason
`secret_canary`) and `mail` (one `mail.mail` to the `base.user_admin` partner). A log-line hit has no database
environment, so only `logging` runs for it.

## Anchor entries

### `hams_secret_canary:fingerprint`

**Purpose.** The only stored form of a secret. `fingerprint(salt, token)`: `salt` `str` (the per-install salt), `token`
`str`. Returns the SHA-256 hex digest (64 characters) of `salt + token` encoded as UTF-8. No errors. Pure. Test:
`TestDetector.test_only_the_hash_is_stored`.

### `hams_secret_canary:new_salt`

**Purpose.** A fresh per-install salt. No arguments; returns 32 random hex characters from `secrets`. Pure. Test:
`TestDetector.test_canaries_are_random_per_call_and_shaped`.

### `hams_secret_canary:make_canary`

**Purpose.** A decoy value. `make_canary(label)`: `label` one of `stripe_secret_key` (`sk_live_` + 24 letters and
digits), `s3_secret_access_key` (40 characters of letters, digits, `/`, `+`) or `cloudflare_api_token` (40 of letters,
digits, `-`, `_`). Returns the value; an unknown label raises `KeyError`. Random per call. Test:
`TestDetector.test_canaries_are_random_per_call_and_shaped`.

### `hams_secret_canary:is_token`

**Purpose.** Whether a value is exactly one token, the only shape the scan can find. `is_token(value)`: `value` any
string. Returns `True` for 16 to 512 characters of `A-Za-z0-9_-+/.` not ending in `.`; `False` otherwise (including
empty). Pure. Test: `TestDetector.test_canaries_are_random_per_call_and_shaped`, `test_registering_a_non_token_fails`.

### `hams_secret_canary:scannable_response`

**Purpose.** Chooses which responses are scanned. `scannable_response(response)`: a werkzeug response. Returns `True`
when it is not `direct_passthrough`, is a sequence (not streamed) and its mimetype is `text/*`, `application/json`,
`application/javascript`, `application/xml` or `application/xhtml+xml`. Pure. Test: `TestResponseHook`.

### `hams_secret_canary:find_matches`

**Purpose.** The scan. `find_matches(text, salt, table)`: `text` `str`; `salt` `str`; `table` `{digest: (label, kind)}`.
Returns a list of `(label, kind)` (`kind` is `canary` or `secret`), each once, in order of first appearance; empty when
none. A token is also tried with leading and trailing dots removed and as each 16+ character piece between slashes (a secret in a URL path). Pure; one hash per distinct token. Test:
`TestDetector.test_secret_in_json_html_and_url_is_found_once_each`, `test_ordinary_responses_have_no_false_positives`,
`test_one_megabyte_scans_fast`.

### `hams_secret_canary:redact`

**Purpose.** Removes values from text. `redact(text, salt, table)`: same arguments. Returns `text` with each matching
token replaced by `[REDACTED:<label>]`. Pure. Test: `TestDetector.test_redact_removes_the_value`.

### `hams_secret_canary:register_secret`

**Purpose.** Registers a secret (or, with `kind="canary"`, a decoy). `hams.secret.canary._register_secret(label, value,
kind="secret")`: `label` `str` shown in alarms; `value` the secret, which must satisfy `is_token` (else `ValueError`);
`kind` `"secret"` or `"canary"`. Stores only the salted hash (creating the salt when there is none) in the
`zero_sudo.kv` entry `hams_secret_canary.state` and drops this process's cached copy; other processes see it within 30
seconds. Writes through `zero_sudo`'s key-value procedure. Private: not callable over RPC. Idempotent for the same
label and value. Test: `TestAlarm`, `TestDetector.test_registering_a_non_token_fails`.

### `hams_secret_canary:load_state`

**Purpose.** Reads the salt and the fingerprint table. `_load_state()` returns `(salt, table)`; `(None, {})` before the
first registration. Cached per process and database for 30 seconds (`CACHE_SECONDS`), so a response costs no query in
the common case; the log watcher reads the cached table. Test: `TestAlarm`.

### `hams_secret_canary:plant_canaries`

**Purpose.** Plants this install's decoys. `_plant_canaries()`: no arguments, no return. For each decoy label not yet
registered and whose `<label>_old` system parameter is empty, generates a value, registers it as a canary and stores it
as that parameter (written as `zero_sudo.config_service_internal`). Idempotent. Test:
`TestPlanting.test_planting_is_idempotent_and_registers_each_decoy`.

### `hams_secret_canary:post_init_hook`

**Purpose.** Runs `_plant_canaries` when the module is installed. `post_init_hook(env)`; returns nothing; idempotent.
Test: `TestPlanting`.

### `hams_secret_canary:scan_text`

**Purpose.** Scan and alarm. `_scan_text(text, source, where)`: `text` `str`; `source` `"response"` or `"log"`; `where`
the request path or logger name (any registered value in it is redacted before use). Returns the `(label, kind)` list;
for each match runs `alerts.raise_alarm`. Runs in the caller's transaction: if a failed request is rolled back, its
`record` and `mail` rows go with it, and the CRITICAL log line stays. Test: `TestAlarm`.

### `hams_secret_canary:post_dispatch_scan`

**Purpose.** The response hook, `ir.http._post_dispatch(response)`. Calls the parent, then, inside a request and for a
scannable response, passes the first `MAX_SCAN_BYTES` (1 MiB) of the body, decoded as UTF-8 with replacement, to
`_scan_text`. Never changes or blocks the response. Test: `TestResponseHook`.

### `hams_secret_canary:register_backend`

**Purpose.** Adds an alert backend. `register_backend(name)` is a decorator for `fn(env, alarm)`; `env` is an Odoo
environment or `None` (log hits); `alarm` an `alerts.Alarm(label, kind, source, where)`. Replaces a backend of the same
name; process-wide. Test: `TestAlarm.test_backend_interface_takes_a_second_backend`.

### `hams_secret_canary:raise_alarm`

**Purpose.** Runs every backend for an alarm, in registration order. `raise_alarm(env, alarm)`. An exception from a
backend propagates (fail fast). Test: `TestAlarm`.

### `hams_secret_canary:logging_backend`

**Purpose.** One CRITICAL line (logger `hams_secret_canary.alarm`): kind, label, source, place. `env` may be `None`.
Test: `TestAlarm`.

### `hams_secret_canary:record_backend`

**Purpose.** A `zero_sudo.security.log` row, reason `secret_canary`, `login` = label, `user_agent` = `source: place`,
created as `zero_sudo.odoo_facility_service_internal`. Does nothing when `env` is `None`. Test: `TestAlarm`.

### `hams_secret_canary:mail_backend`

**Purpose.** One `mail.mail` (subject, label, source, place; no value) to the `base.user_admin` partner's address,
created as `zero_sudo.mail_service_internal` and sent by the normal mail queue. Logs a warning and queues nothing when
the administrator has no address; does nothing when `env` is `None`. Test: `TestAlarm`.
