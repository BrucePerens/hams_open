<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: A Leaked Secret Is Noticed

As the **Operator** of an Odoo server whose web process can read every secret it uses, I want a decoy credential or a
registered real secret that appears in a response or a log line to raise an alarm, so that the next bug that leaks a
secret is noticed when it happens and not months later.

1. When the module is installed, a per-install salt is generated and a decoy Stripe key, S3 secret and Cloudflare token
   are generated and stored as the system parameters `stripe_secret_key_old`, `s3_secret_access_key_old` and
   `cloudflare_api_token_old`. Their salted hashes are the registered canaries.
   `hams_secret_canary:post_init_hook` `hams_secret_canary:plant_canaries`
   `hams_secret_canary:new_salt` `hams_secret_canary:make_canary`
   The same decoys are also set as environment variables and written to files in the `keys` directory of the data
   directory. `hams_secret_canary:plant_decoy_file_and_env`
2. Code that holds a real secret registers it; only the salted hash is stored.
   `hams_secret_canary:register_secret` `hams_secret_canary:fingerprint`
   `hams_secret_canary:is_token` `hams_secret_canary:load_state` `hams_secret_canary:store_state`
3. Every finished text response is scanned for a registered token, and every log record is scanned as it is made.
   `hams_secret_canary:post_dispatch_scan` `hams_secret_canary:scannable_response`
   `hams_secret_canary:scan_text` `hams_secret_canary:find_matches` `hams_secret_canary:candidates`
   `hams_secret_canary:register_hook` `hams_secret_canary:watch_log_records`
   A response a controller builds itself, such as an XML-RPC fault, is scanned by a WSGI wrapper.
   `hams_secret_canary:scan_wsgi` `hams_secret_canary:scan_after_request`
   The wrapper scans only text media types, passes the body through unchanged and closes it, and looks at the first megabyte.
   `hams_secret_canary:is_text` `hams_secret_canary:pass_through` `hams_secret_canary:scan_chunks`
4. A hit runs every alert backend: a CRITICAL log line, a security-log row and an email to the administrator. A log
   line that held the value is rewritten with the value removed. No alarm carries the matched value.
   `hams_secret_canary:redact` `hams_secret_canary:raise_alarm`
   `hams_secret_canary:register_backend` `hams_secret_canary:logging_backend`
   `hams_secret_canary:record_backend` `hams_secret_canary:mail_backend`

This detects a leak after it has happened. It does not prevent one.

## Anchor reference

One entry per anchor named in the story, read from the code of `hams_secret_canary` (`detector.py`, `alerts.py`, `wsgi_scan.py`, `hooks.py`, `models/`, `tests/test_canary.py`). Every function in the module is covered; the module keeps no data structure of its own beyond the `Alarm` record and the stored state described under `hams_secret_canary:load_state`.

### `hams_secret_canary:post_init_hook` [@ANCHOR: hams_secret_canary:post_init_hook]

`post_init_hook(env)` in `hooks.py`, named as the module's post-install hook. Called once by Odoo after the module installs, with the installing user's environment. It calls `env["hams.secret.canary"]._plant_canaries()` and returns nothing. It is idempotent: an installation that already has a salt and decoys keeps them, so an upgrade or reinstall does not rotate the canaries. Any exception from planting aborts the install (fail fast). Test: `test_decoys_are_also_an_environment_variable_and_a_private_file`. See `hams_secret_canary:plant_canaries`.

### `hams_secret_canary:plant_canaries` [@ANCHOR: hams_secret_canary:plant_canaries]

`hams.secret.canary._plant_canaries(self)` (`@api.model`). Loads the saved state, then for each label in `detector.CANARY_SHAPES` (`stripe_secret_key`, `s3_secret_access_key`, `cloudflare_api_token`) that is not already registered as a canary and whose system parameter `<label>_old` is empty it generates a decoy with `make_canary`, registers its fingerprint with kind `canary` and stores the decoy value in the parameter `<label>_old`. Existing parameters of that name are left alone and are not registered. It runs as the installing user, because a service account may not read or write those parameters. It finishes by calling `_plant_decoy_file_and_env`. Returns nothing; safe to call repeatedly. The plaintext decoy is stored in the parameter by design, since the decoy is meant to be found and used by an attacker. Test: `test_planting_is_idempotent_and_registers_each_decoy`.

### `hams_secret_canary:new_salt` [@ANCHOR: hams_secret_canary:new_salt]

`new_salt()` in `detector.py`. No arguments. Returns a fresh random per-install salt as 32 hexadecimal characters (`secrets.token_hex(16)`). Called by `_register_secret` the first time anything is registered. The salt is stored in the state under the key `hams_secret_canary.state` next to the table of digests; it makes the stored hashes unique to the installation so they cannot be matched against a precomputed list of well-known keys. Pure function, no Odoo, no I/O. Test: `test_canaries_are_random_per_call_and_shaped` and the detector test that cites it.

### `hams_secret_canary:make_canary` [@ANCHOR: hams_secret_canary:make_canary]

`make_canary(label)` in `detector.py`. `label` must be a key of `CANARY_SHAPES`: `stripe_secret_key` (prefix `sk_live_` plus 24 letters and digits), `s3_secret_access_key` (40 characters from letters, digits, `/` and `+`) or `cloudflare_api_token` (40 characters from letters, digits, `-` and `_`). Returns a new random decoy string of that shape using `secrets.choice`, so two calls differ. Raises `KeyError` for an unknown label. No side effects. The shapes are plausible look-alikes of the credentials an attacker looks for first. Test: `test_canaries_are_random_per_call_and_shaped`. See `hams_secret_canary:plant_canaries`.

### `hams_secret_canary:plant_decoy_file_and_env` [@ANCHOR: hams_secret_canary:plant_decoy_file_and_env]

`hams.secret.canary._plant_decoy_file_and_env(self)` (`@api.model`). For each canary label whose `<label>_old` system parameter is set, it sets the environment variable `<LABEL>_OLD` in this process (not overriding an existing one) and writes the value to a file named `<label>_old` in the `keys` directory of Odoo's `data_dir` (created with mode 0700; file mode 0600, created exclusively so an existing file is never replaced). These are the places a leaking bug or an intruder reads. The values are the same canaries already registered, so a leak of any copy is noticed. Best effort: an `OSError` is logged with only its type name and skipped. Called from `_plant_canaries` and from `_register_hook`. Returns nothing. Test: `test_decoys_are_also_an_environment_variable_and_a_private_file`.

### `hams_secret_canary:register_secret` [@ANCHOR: hams_secret_canary:register_secret]

`hams.secret.canary._register_secret(self, label, value, kind="secret")` (`@api.model`, private, not callable over RPC). Registers `value` so that its later appearance in a response or a log line raises an alarm.

| Argument | Type | Meaning |
|---|---|---|
| `label` | str | Name shown in alarms; never the value. |
| `value` | str | Must be exactly one token (`is_token`): 16 to 512 characters of letters, digits and `_-+/.`. |
| `kind` | str | `"secret"` (default, a real secret) or `"canary"` (a decoy). |

It loads the state, creates a salt if there is none, adds the entry `{fingerprint(salt, value): (label, kind)}` and saves it with `_store_state`, so only the salted SHA-256 is stored. Raises `ValueError` when `value` is not a token. Not concurrency safe: two simultaneous registrations can lose one (read-modify-write of one key-value row). Test: `test_registering_a_non_token_fails`, `test_only_the_hash_is_stored`.

### `hams_secret_canary:fingerprint` [@ANCHOR: hams_secret_canary:fingerprint]

`fingerprint(salt, token)` in `detector.py`. Returns the hexadecimal SHA-256 digest of the UTF-8 encoding of `salt + token`. It is the only form in which a secret is stored, and the key used to look a candidate up in the scan table. Both arguments are `str`; there is no error handling (a non-string raises `TypeError`). Pure and deterministic, about one hash of the token length per call. A plain salted hash is deliberate: tokens are high-entropy, so no slow key-stretching is needed. Test: `test_only_the_hash_is_stored`.

### `hams_secret_canary:is_token` [@ANCHOR: hams_secret_canary:is_token]

`is_token(value)` in `detector.py`. Returns True when `value` is non-empty, matches the whole of `TOKEN_RE` (`[A-Za-z0-9_\-+/.]{16,512}`, one run of letters, digits and `_ - + / .`), and does not end with a dot. That is the only shape the scan can find, because the scan splits text into such runs. `_register_secret` uses it to refuse values that could never be detected (spaces, quotes, too short or too long). Returns False, never raises. Test: `test_registering_a_non_token_fails`.

### `hams_secret_canary:load_state` [@ANCHOR: hams_secret_canary:load_state]

`hams.secret.canary._load_state(self)` (`@api.model`). Returns `(salt, table)` for the current database: `salt` is a string or `None` before the post-install hook has run, and `table` maps digest to a `(label, kind)` tuple. It reads the JSON stored under the key `hams_secret_canary.state` in the `zero_sudo` key-value store (`_get_kv`) and caches the result per database in the module-level `_tables` for 30 seconds (`CACHE_SECONDS`, monotonic clock), so a registration made in another worker is seen within about half a minute. The log watcher reads whatever the cache holds. No locks; the cache is per process. Errors: a corrupt stored value raises `json.JSONDecodeError`. Test: `test_secret_in_a_response_body_raises_the_alarm_without_the_value`.

### `hams_secret_canary:store_state` [@ANCHOR: hams_secret_canary:store_state]

`hams.secret.canary._store_state(self, salt, table)` (`@api.model`). Writes `{"salt": salt, "table": table}` as JSON to the `zero_sudo` key-value store under `hams_secret_canary.state` with `_set_kv`, then drops this database's cache entry so the next `_load_state` rereads it. The table must hold only digests and `(label, kind)` pairs, never secrets. Returns nothing. The write joins the caller's transaction, so a rollback also rolls back the registration. Last writer wins when two workers store at once. Test: `test_only_the_hash_is_stored`.

### `hams_secret_canary:post_dispatch_scan` [@ANCHOR: hams_secret_canary:post_dispatch_scan]

`IrHttp._post_dispatch(cls, response)` (classmethod override in `models/ir_http.py`). Calls `super()`, then, if there is a request and `scannable_response(response)` is true, sets `environ["hams_secret_canary.scanned"] = True` (so the WSGI wrapper does not scan the same response again), takes the first 1 MiB of the body (`MAX_SCAN_BYTES`), decodes it as UTF-8 with replacement and calls `_scan_text(text, "response", path)`. Returns what `super()` returned and never changes the response: the leak has already happened, this only makes it noticed. Streamed, passthrough and non-text responses are skipped. Alert-backend exceptions propagate and would fail the request (fail fast). Test: `test_a_response_carrying_a_registered_token_raises_the_alarm`.

### `hams_secret_canary:scannable_response` [@ANCHOR: hams_secret_canary:scannable_response]

`scannable_response(response)` in `detector.py`. Takes a werkzeug response and returns True for a finished, non-streamed text response: `direct_passthrough` is false, the body is a sequence (`is_sequence`), and the mimetype starts with `text/` or is one of `application/json`, `application/javascript`, `application/xml` or `application/xhtml+xml`. A missing mimetype counts as empty and gives False. Reads no body, so it is cheap and cannot consume a stream. Used by `_post_dispatch`. Test: `test_a_response_carrying_a_registered_token_raises_the_alarm`.

### `hams_secret_canary:scan_text` [@ANCHOR: hams_secret_canary:scan_text]

`hams.secret.canary._scan_text(self, text, source, where)` (`@api.model`). Looks for registered fingerprints in `text`.

| Argument | Meaning |
|---|---|
| `text` | The text to scan (a response body prefix). |
| `source` | `"response"` or `"log"`, recorded in the alarm. |
| `where` | The request path; any registered token in it is redacted before use. |

Returns the list of `(label, kind)` found, empty when nothing is registered yet (so a fresh install pays nothing). For each match it calls `alerts.raise_alarm(self.env, Alarm(label, kind, source, where))`. Errors from backends propagate. Costs one hash per distinct token-shaped run in `text`. No match ever carries the matched value. Test: `test_secret_in_a_response_body_raises_the_alarm_without_the_value`.

### `hams_secret_canary:find_matches` [@ANCHOR: hams_secret_canary:find_matches]

`find_matches(text, salt, table)` in `detector.py`. Splits `text` into token-shaped runs (`TOKEN_RE`), and for each distinct run hashes it and its candidates (`_candidates`) with `fingerprint(salt, candidate)` and looks the digest up in `table` (`{digest: (label, kind)}`). Returns the list of `(label, kind)` pairs found, each once, ordered by first appearance in the text. Cost is one hash per distinct candidate. Pure function; arguments are not modified. It finds a secret that is delimited by quotes, spaces, punctuation or URL separators, but not one split by other characters. Test: `test_secret_in_json_html_and_url_is_found_once_each`, `test_ordinary_responses_have_no_false_positives`.

### `hams_secret_canary:candidates` [@ANCHOR: hams_secret_canary:candidates]

`_candidates(token)` in `detector.py`, a generator of the strings worth hashing for one token-shaped run. It yields the token itself, then the token with leading and trailing dots stripped and each piece between slashes, but only pieces of at least 16 characters and each once. That makes a secret found when it ends a sentence (`key.`) or sits inside a path (`/a/<secret>/b`). It has no side effects and does not hash anything itself. Used by `find_matches` and `redact`. Test: `test_secret_in_json_html_and_url_is_found_once_each`.

### `hams_secret_canary:register_hook` [@ANCHOR: hams_secret_canary:register_hook]

`hams.secret.canary._register_hook(self)` (`@api.model`), Odoo's per-registry-load hook. Calls `super()`, `_load_state()` and `_plant_decoy_file_and_env()`, then installs two process-wide wrappers once (guarded by the module lists `_original_factory` and `_original_call`, so a second registry load does not stack them): a log-record factory (`_watch_log_records`) set with `logging.setLogRecordFactory`, and a replacement of `http.Application.__call__` with `wsgi_scan.scan_wsgi(original, scan_after_request)`. Returns nothing. Side effects are global to the Python process, in all databases, and are never undone. Test: `test_a_log_line_with_a_registered_secret_raises_the_alarm_and_is_redacted`.

### `hams_secret_canary:watch_log_records` [@ANCHOR: hams_secret_canary:watch_log_records]

`_watch_log_records(factory)` in `models/secret_canary.py`. Takes the existing log-record factory and returns a replacement `make_record(*args, **kwargs)`. For every record created, if any database state is loaded and the logger name does not start with `hams_secret_canary` (so the alarm line cannot trigger itself), it formats the message and calls `find_matches` against each loaded database's table. On a match it rewrites the record with `redact` (the value becomes `[REDACTED:<label>]`, `args` cleared) and calls `alerts.logging_backend(None, Alarm(label, kind, "log", logger_name))`, then stops after the first database that matched. It uses only the cached tables, never touching the database, and so is safe inside logging. It adds a hash per token on every log call of the process. It never makes a logging call fail: a database with no salt or no fingerprints is skipped, and any exception while formatting or scanning (a malformed call such as `logger.info("%s %s", one_argument)`, which logging alone survives) leaves the record as it was made. Tests: `test_a_log_line_with_a_registered_secret_raises_the_alarm_and_is_redacted`, `test_logging_never_raises_in_the_caller_whatever_the_record_or_the_state`.

### `hams_secret_canary:scan_wsgi` [@ANCHOR: hams_secret_canary:scan_wsgi]

`scan_wsgi(application, scan)` in `wsgi_scan.py`. Wraps a WSGI application and returns a callable taking `(environ, start_response)` or `(self, environ, start_response)` (it is installed over a method). It runs `application`, remembers the response `Content-Type`, and returns the body unchanged unless the type is text (`_is_text`) and `environ["hams_secret_canary.scanned"]` is not set; then it returns `_pass_through(body, path, scan)`. After the body is fully sent, `scan(text, path)` is called with the first `MAX_SCAN_BYTES` bytes decoded. A failure in `scan` is handled by the caller, not the client. It covers responses that never pass `ir.http._post_dispatch`, such as an XML-RPC fault built from an exception. Test: `test_text_is_scanned_after_it_is_sent_unchanged_even_when_split_in_chunks`.

### `hams_secret_canary:scan_after_request` [@ANCHOR: hams_secret_canary:scan_after_request]

`scan_after_request(text, where)` in `models/secret_canary.py`, the `scan` callback of `scan_wsgi`. For every database whose table this process has loaded it runs `find_matches` on `text`; on a match it redacts `where`, opens a fresh cursor with `Registry(dbname).cursor()` (the request's own is gone), resolves the `zero_sudo.odoo_facility_service_internal` user id with the SQL function `zero_sudo_get_service_uid`, builds an environment for it and calls `alerts.raise_alarm` for each match with source `"response"`. Any exception is logged and swallowed (`audit-ignore-catch-all`: a detector must not break the response). Returns nothing. Test: `test_a_fault_naming_a_registered_secret_raises_the_alarm_once_without_the_value`.

### `hams_secret_canary:is_text` [@ANCHOR: hams_secret_canary:is_text]

`_is_text(content_type)` in `wsgi_scan.py`. Takes a `Content-Type` header value, cuts off parameters after `;`, lower-cases and trims it, and returns True when it starts with `text/` or is one of `detector.SCANNABLE_MIMETYPES` (JSON, JavaScript, XML, XHTML). Used by `scan_wsgi` to skip binary bodies such as images and downloads. No side effects. It is the header-string twin of `detector.scannable_response`. Tests: `test_binary_responses_are_not_scanned` and the chunk-scan test.

### `hams_secret_canary:pass_through` [@ANCHOR: hams_secret_canary:pass_through]

`_pass_through(body, path, scan)` in `wsgi_scan.py`. Returns an iterable that yields exactly the chunks of the WSGI `body`, scanning them on the way through (`_scan_chunks`), and that closes `body` when the server closes the result (werkzeug `ClosingIterator`). The client sees the identical bytes in the identical chunks; scanning happens after the last chunk has been yielded. It keeps at most 1 MiB in memory. Errors raised by the original body propagate to the server as before. Test: `test_text_is_scanned_after_it_is_sent_unchanged_even_when_split_in_chunks`.

### `hams_secret_canary:scan_chunks` [@ANCHOR: hams_secret_canary:scan_chunks]

`_scan_chunks(chunks, path, scan)` in `wsgi_scan.py`, a generator. For each chunk it appends up to the remainder of `MAX_SCAN_BYTES` (1 MiB) to a buffer and yields the chunk untouched; once the iterable is exhausted it decodes the buffer as UTF-8 with replacement and calls `scan(text, path)`. A secret split across two chunks is still found because the whole prefix is joined first. Bytes after the first megabyte are not scanned, bounding the cost. Test: `test_only_the_first_megabyte_is_scanned`, `test_text_is_scanned_after_it_is_sent_unchanged_even_when_split_in_chunks`.

### `hams_secret_canary:redact` [@ANCHOR: hams_secret_canary:redact]

`redact(text, salt, table)` in `detector.py`. Returns `text` with every token-shaped run (or the piece of it that `_candidates` matched) whose fingerprint is in `table` replaced by `[REDACTED:<label>]`; everything else is unchanged. Used so that the log line and the request path in an alarm never contain the value. Arguments are not modified. Cost is one hash per candidate per token. It cannot remove a secret that does not look like a token or that was split by other characters. Test: `test_redact_removes_the_value`.

### `hams_secret_canary:raise_alarm` [@ANCHOR: hams_secret_canary:raise_alarm]

`raise_alarm(env, alarm)` in `alerts.py`. Calls every registered backend, in registration order, as `backend(env, alarm)`. `alarm` is a frozen `Alarm(label, kind, source, where)`, where `source` is `"response"` or `"log"` and `where` is a request path or logger name already redacted; it never holds the matched value. An exception from a backend propagates and later backends do not run (fail fast). `env` may be `None` (the log watcher has no environment): backends that need one skip themselves. Returns nothing. Test: `test_secret_in_a_response_body_raises_the_alarm_without_the_value`.

### `hams_secret_canary:register_backend` [@ANCHOR: hams_secret_canary:register_backend]

`register_backend(name)` in `alerts.py`, a decorator factory. `@register_backend("x")` stores the decorated function `fn(env, alarm)` in the module dictionary `BACKENDS` under `x`, replacing any backend of that name, and returns the function unchanged. The three shipped backends are `logging`, `record` and `mail`; another module can add its own (a pager, a webhook) the same way and it runs for every alarm. Not thread-locked: register at import time. Test: `test_backend_interface_takes_a_second_backend`.

### `hams_secret_canary:logging_backend` [@ANCHOR: hams_secret_canary:logging_backend]

`logging_backend(env, alarm)` in `alerts.py`, registered as `logging`. Writes one CRITICAL line on the logger `hams_secret_canary.alarm`: "SECRET CANARY TRIPPED: <kind> <label> seen in <source> (<where>)". `env` is ignored and may be `None`, which is how the log-record watcher calls it. The line is excluded from the watcher by its logger name prefix, so it cannot recurse. It carries no secret value. Returns nothing. Test: `test_a_log_line_with_a_registered_secret_raises_the_alarm_and_is_redacted`.

### `hams_secret_canary:record_backend` [@ANCHOR: hams_secret_canary:record_backend]

`record_backend(env, alarm)` in `alerts.py`, registered as `record`. Does nothing when `env` is `None`. Otherwise it resolves the facility service account `zero_sudo.odoo_facility_service_internal` and creates one `zero_sudo.security.log` row with `reason = "secret_canary"` (a selection value this module adds), `login` set to the alarm label and `user_agent` set to `"<source>: <where>"`. The row joins the caller's transaction. Errors, such as a missing service account, propagate. No value is written. Test: `test_secret_in_a_response_body_raises_the_alarm_without_the_value`.

### `hams_secret_canary:mail_backend` [@ANCHOR: hams_secret_canary:mail_backend]

`mail_backend(env, alarm)` in `alerts.py`, registered as `mail`. Does nothing when `env` is `None`. Otherwise, as the `zero_sudo.mail_service_internal` account it reads the address of `base.user_admin`'s partner; with none it logs a warning and sends nothing. With an address it creates one `mail.mail` ("Secret canary tripped: <label>", a short HTML body naming kind, label, source and place, `auto_delete` false) which Odoo's normal mail queue sends later. The body states that the alarm detects a leak after it happened and carries no value. Test: `test_secret_in_a_response_body_raises_the_alarm_without_the_value`.
