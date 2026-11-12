# Zero-Sudo Security Core [@ANCHOR: zero_sudo_main] (`zero_sudo`)

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).*

This module is the foundational security layer for our Odoo ecosystem. It enforces a strict **Zero-Sudo Architecture** (Architecture Decision Record ADR-0002) to prevent privilege escalation vulnerabilities and physically isolates background service accounts from interactive web sessions (ADR-0005). ADR-0002 and ADR-0005 are consolidated in `hams_shared/docs/adrs/MASTER_01_SECURITY_ZERO_SUDO.md`.

## 🛡️ Core Security Missions

1.  **Eliminate `.sudo()` Usage:** Developers are forbidden from using Odoo's native `.sudo()` command, which switches the environment into superuser mode (`env.su`) for the whole call, so access rules are no longer applied; "Zero-Sudo" means none of it. This module provides a secure, audited alternative via the **Service Account Pattern**.
2.  **Web Isolation:** Automated background tasks (Service Accounts) are strictly prohibited from logging into the website interface, preventing session hijacking or accidental human interference.
3.  **Mechanical Whitelisting:** Critical system parameters (like cryptographic keys) are blocked from being accessed or modified through the Zero-Sudo parameter helpers (`_get_system_param` / `_set_system_param`) unless they are explicitly registered in a secure code-level whitelist.
4.  **Security Audit Trail:** Blocked service-account logins, denied system-parameter reads and writes, and model cache invalidations are logged for administrator review.
5.  **High-Performance Security Interop:** Critical security lookups and atomic key-value (KV) operations are optimized via PostgreSQL procedures to minimize latency and ensure transaction-safe execution.

---

## 📖 User Guide (Non-Technical)

### What is a Service Account?
A "Service Account" is a special user profile used only by background programs, bots, and automated tasks. Because these accounts often have powerful permissions, they are **forbidden** from being used to log into the website through a browser.

### Managing Service Accounts:
1.  **Restricting an Account:** Go to **Settings > Users**, select a user, and check the **"Is Service Account"** box on the **Access Rights** tab. Only Settings administrators (`base.group_system`) can see or change this box.
2.  **Access Denied:** Once flagged, any login attempt with that account via the browser will result in an "Access Denied" error. This is a deliberate security feature.
3.  **Automatic Protection:** Every Service Account is automatically assigned an extremely long, randomized password that is impossible to guess, ensuring it can only be accessed by the automated system intended to use it. Turning the box on or off replaces the password with a new random one, and later password changes to a Service Account are ignored.

### Security Audit Logs:
Administrators can monitor security events in **Settings > Zero-Sudo Security > Security Audit Logs**. The following events are tracked:
*   **Service Account Web Login Attempt:** Logs when a background account signs in to the web interface with its correct password (the session is then ended).
*   **God-Mode Security Block Tripped:** A reserved event type for a background account trying to use global administrative privileges. Nothing in this repository currently records it; a blocked attempt is refused with an access error to the program that asked instead.
*   **Unauthorized System Parameter Access:** Logs when a process attempts to read a restricted system setting.
*   **Unauthorized System Parameter Write:** Logs when a process attempts to change a restricted system setting.
*   **Model Cache Invalidation:** Logs when a user or process manually clears the cached data for one kind of record (one model), not the whole system.

---

# Technical Documentation

<system_role>
**Context:** Technical documentation strictly for LLMs and Integrators developing custom downstream modules...
</system_role>

<architecture>
## Core Architecture

1.  **Service Account Pattern**: High-privilege operations are offloaded to dedicated `res.users` records flagged with `is_service_account=True`.
2.  **Centralized Security Utilities**: The `zero_sudo.security.utils` model provides cached methods for secure UID retrieval, system parameter whitelisting, and deterministic hashing.
3.  **Web Isolation Interceptor**: A controller override on `web_login` that uses raw SQL to check and block interactive logins for service accounts. The JSON login route (`/web/session/authenticate`) is overridden the same way, and an `ir.http._authenticate` check refuses every later request from a service-account session except the RPC (remote procedure call) endpoints `/jsonrpc` and `/xmlrpc/...`.
4.  **Automated Documentation Bootstrap**: An inheritance of `ir.module.module` that centrally manages the installation of HTML documentation from module manifests. On every registry load it installs or updates each installed module's `knowledge_docs` entries as `knowledge.article` records, skipping a document whose content hash is unchanged, and does nothing when the `knowledge.article` model is not installed. Documents are visible only to administrators unless their manifest entry sets `"public": True`.
</architecture>

<security_design>
## Security Design (ADR-0002, ADR-0005)

-   **Anti-IDOR (Insecure Direct Object Reference) & Privilege Escalation**: `_get_service_uid` performing direct SQL lookups (the PL/pgSQL functions in `data/postgres_procedures.xml`) strictly rejects any account that is disabled, is not flagged as a service account, or holds a global administrative group (`base.group_system`, `base.group_erp_manager`), directly or through implied groups. A rejection raises `AccessError`.
-   **Mechanical Secret Block**: `_get_system_param` and `_set_system_param` enforce hardcoded whitelists (`_get_param_read_whitelist()` and `_get_param_write_whitelist()`) to prevent SSTI (Server-Side Template Injection) exfiltration. Any key not on the list is denied with `AccessError` and logged. For a denied read, a key matching a cryptographic pattern (e.g., "secret", "token") only changes the error message; the pattern check does not apply to whitelisted keys, some of which are credentials.
</security_design>

---

<service_account_pattern>
## 1. The Service Account Pattern

You are strictly FORBIDDEN from using `.sudo()` inline. To escalate privileges:
1. Define your service account in your module's XML data and set `<field name="is_service_account" eval="True"/>`.
2. Retrieve its UID securely:
   `svc_uid = self.env['zero_sudo.security.utils']._get_service_uid('your_module.user_xml_id')`
3. Execute using the impersonation idiom:
   `self.env['target.model'].with_user(svc_uid).create(vals)`
</service_account_pattern>

---

<shared_service_accounts>
## 2. Centralized Shared Service Accounts

When a daemon strictly requires native ERP framework interactions that mandate `base.group_user`, they MUST temporarily assume one of the two centralized proxy accounts. These are the only two accounts this module declares with `base.group_user`; its other service accounts (`config_service_internal`, `gdpr_service_internal`, `user_lockout_service_internal`, and the never-acting `orphaned_record_owner`) hold only narrow groups or none:

### A. Central Mail Service Account (`zero_sudo.mail_service_internal`)
* **Use Case:** Execute `message_post()`, `send_mail()`, or interact with the `mail.thread` chatter.

### B. Odoo Facility Service Account (`zero_sudo.odoo_facility_service_internal`)
* **Use Case:** Complex ORM cascades that deeply assume internal user rights.
</shared_service_accounts>

---

<python_api>
## 3. Python API Reference (`zero_sudo.security.utils`)

#### `_get_service_uid(xml_id)` `[@ANCHOR: get_service_uid]`
Safely retrieves the database ID of a Service Account. Only the XML ID to user ID lookup is RAM-cached (preloaded at registry load). The safety check (account active, flagged as a service account, no administrative groups) runs live on every call, and a failure raises `AccessError`.
* **Arguments:** `xml_id` (str): The external ID (e.g., `'your_module.your_service_account'`).
* **Returns:** `int` (The User ID).

**Start-up preload** `[@ANCHOR: zero_sudo:register_hook_preload_service_uids]` `[@ANCHOR: zero_sudo:preload_service_uid_cache]`. The `_register_hook()` of `zero_sudo.security.utils` runs once for each registry build and calls `_preload_service_uid_cache()`. That method reads every `ir_model_data` row of model `res.users` whose user has `is_service_account` set and stores `(database name, xml_id) -> uid` in a process-wide dict. On the production configuration the registry is built in the master process before any worker forks, so every worker inherits the filled table. Only the resolution is cached, never the verdict. Entries are keyed by database so a multi-database deployment cannot mix them up, and a reload replaces only the entries of the database being loaded. A service account that did not exist at load time falls back to the full lookup and is deliberately not written back, because the dict is not transaction-scoped and a rolled-back test would otherwise leak into later calls.

**No default-env flush** `[@ANCHOR: zero_sudo:COMM_get_service_uid_no_transaction_flush]`. The live check in `_get_service_uid` runs its SELECT inside a savepoint opened with `flush=False`, after flushing through the caller's own environment. The default savepoint flushes the transaction's `default_env`, and a public request has none yet when a service account is resolved before authentication (the case on every anonymous page view), which made Odoo log "Missing default_env, flushing as public user" and look up the public user each time. Flushing the caller's environment still makes a pending ORM write to the service user visible to the check.

#### `_get_deterministic_hash(input_string)` `[@ANCHOR: deterministic_hash]`
Generates a deterministic 31-bit integer hash for `pg_advisory_xact_lock`, so every worker that locks the same logical resource computes the same lock number.

| Argument | Type | Meaning |
|---|---|---|
| `input_string` | `str` (anything else is converted with `str()`) | the name of the resource to lock, for example a model and key |

* **Returns:** `int` from 0 to 2147483646: the first 8 hex digits of the SHA-256 of the UTF-8 text, modulo 2147483647. The same text always gives the same number, in every process and on every host (Python's built-in `hash()` is not used because it is randomised per process).
* **Errors:** none. Pure function: no database access, no lock taken; the caller passes the number to `pg_advisory_xact_lock`.
* **Special concerns:** two different names can collide (a 31-bit space), which only means two unrelated operations wait for each other, never that the lock fails. Not a security hash: do not use it for anything secret.
* **Tests:** `test_06_get_deterministic_hash` in `tests/test_security_utils.py`.

#### `_get_system_param(key, default=None)` `[@ANCHOR: get_system_param]`
Safely retrieves a whitelisted system configuration parameter, reading it as `zero_sudo.config_service_internal`. A key not on the read whitelist is logged as `param_access_denied` and raises `AccessError`.

| Argument | Type | Meaning |
|---|---|---|
| `key` | `str` | the `ir.config_parameter` key; it must be in `_get_param_read_whitelist()` |
| `default` | any | returned when the parameter is not set |

* **Returns:** the stored value (a `str`) or `default`. The result goes through the distributed cache decorator, so it can be a cached value.
* **Errors:** `AccessError` for a key not on the whitelist. When the key also contains `secret`, `key`, `password`, `token`, `auth`, `crypt` or `cert` the message says it matches restricted cryptographic patterns (the mechanical secret block); either way a `zero_sudo.security.log` row with reason `param_access_denied` is written first, through the facility service account, with the key in the login column.
* **Security:** the whitelist and the banned-substring check are this function's boundary; the read itself is done as `zero_sudo.config_service_internal`, never with `.sudo()`. A secret belongs in the secret store, never in a whitelisted parameter. Writing is `_set_system_param`, which has its own, shorter write whitelist and logs `param_write_denied`.
* **Tests:** `tests/test_security_utils.py` (the whitelist and the secret block).

#### `_notify_cache_invalidation(model_name, key_value)` `[@ANCHOR: coherent_cache_signal]`
Emits a PostgreSQL `NOTIFY` event to synchronize distributed caches. It delegates to `distributed_redis_cache`'s `notify_model_invalidation()`, which signals on the `distributed_cache_invalidation` channel that the `distributed_redis_cache` cache-manager daemon listens on. Invalidation is for the whole model; `key_value` must be non-empty but does not narrow it. What this signal does and does not reach on other workers is spelled out in [docs/stories/cache_signaling.md](docs/stories/cache_signaling.md).

#### `_get_crypto_secret()` `[@ANCHOR: get_crypto_secret]`
Retrieves the root cryptographic key from environment or local file, bypassing DB. It checks the `HAMS_CRYPTO_KEY` environment variable, then `/var/lib/odoo/hams_crypto.secret`, then Odoo's `admin_passwd` setting. If none is set, or the value is `admin`, it logs an error and returns an empty string, so callers must refuse to encrypt or sign.

#### `_invalidate_model_cache(model_name)` `[@ANCHOR: invalidate_model_cache]`
Securely invalidates the entire cache for a specific model. Requires the user to have write access to the model (members of `base.group_system` skip this check). It clears that model's entries in Redis and in this worker's local cache, notifies the other workers through `_notify_cache_invalidation`, and records a `cache_invalidation` security log entry.

#### `_set_kv(key, value)` `[@ANCHOR: set_kv_procedure]`
High-performance atomic key-value update using an optimized Postgres procedure (`zero_sudo_set_kv`, a single `INSERT ... ON CONFLICT (key) DO UPDATE` upsert).

| Argument | Type | Meaning |
|---|---|---|
| `key` | `str` | the key in `zero_sudo.kv` (unique) |
| `value` | `str` | the text to store; replaces any previous value |

* **Returns:** nothing. A missing key is created, an existing one is overwritten, in one statement, so there is no read-then-write race between workers and no Python-side existence check.
* **Side effects:** the direct SQL bypasses the ORM cache, so the method then invalidates the `zero_sudo.kv` model, clears this worker's cache entries for `zero_sudo.security.utils` and sends the cross-worker invalidation signal, so a following `_get_kv` on any worker reads the new value. It does not commit; the write belongs to the caller's transaction.
* **Errors:** a database error propagates. The procedure is installed by `data/postgres_procedures.xml` when the module is installed or updated.
* **Tests:** `tests/test_security_utils.py` (the procedure and the cache invalidation).
</python_api>

---

<tour_utils>
## 4. UI Tour Macros (`tour_utils.js`)

The `TourUtils` object (`@zero_sudo/js/tour_utils`) provides centralized macros for Odoo UI Tours to guarantee architectural compliance and eliminate race conditions in headless browser testing.

#### `safeSave(saveButtonTrigger, waitTrigger)`
Generates a step array that clicks a save button and then waits (a native tour step, no polling loop) for `.o_form_saved`, the form renderer's own "persisted and not dirty" class, so the tour cannot end or navigate while the save RPC is still in flight. Pass `waitTrigger` to wait on something else instead. (Do not wait on `.o_form_button_create`: the control panel's always-present New button matches before the save resolves.)

#### `bypassDialogs()`
Generates a macro to intercept and bypass native blocking dialogs (`window.alert`, `window.confirm`) during testing. Intercepted dialogs are logged as warnings, and `confirm` always answers yes.

#### `mockExternalRequests(urlPattern, mockResponse)`
Generates a macro to intercept and mock `fetch` requests matching a specific URL pattern, preventing external network calls during tests. The pattern is a plain substring (not a regular expression); a matching request gets `mockResponse` as a JSON body with status 200.

#### `waitForAbsence(selector, description)`
Generates a macro that polls and explicitly waits for a specific DOM element to disappear from the page. It checks every 250 ms and fails the step after 10 seconds.

#### `deterministicInput(helpers, text)`
A direct execution function (used within a tour's `run` step) that safely injects text into the currently active input element and explicitly dispatches the `input`, `change`, and `keyup` events required to reliably awaken Odoo's frontend framework (e.g., Many2one debouncers).
</tour_utils>

## 5. Additional Models and Utilities

### Inline SVG allowlist sanitizer (`svg_sanitizer.py`) `[@ANCHOR: zero_sudo:svg_allowlist_sanitizer]`
Odoo's HTML sanitizer strips every SVG shape, so inline schematics render as empty boxes. At import
time this module patches `odoo.tools.mail._Cleaner.__call__` (installed from `__init__.py`,
`[@ANCHOR: zero_sudo:svg_allowlist_install]`) so every `fields.Html` write, and every module that
imported `html_sanitize` by name, rebuilds each `<svg>` from a strict element/attribute/value
allowlist and leaves the rest of the document to the stock sanitizer. Design, threat model,
residual risks and the vector list: `docs/stories/svg_allowlist_sanitizer.md`. Tests:
`tests/test_svg_sanitizer.py`, shared corpus `tests/svg_corpus.py`.

### Sanitizer helper functions (`css_sanitizer.py`, `svg_sanitizer.py`)
Plumbing behind the two member-content filters above; callers use only `sanitize_stylesheet`, `sanitize_style_attribute`, `sanitize_svg_element`, `sanitize_xml_svgs` and the `install()` hook. All are pure functions of their arguments unless noted, take no locks and do no I/O. Tests for every entry: `tests/test_sanitizer_functions.py`.

#### `css_sanitizer.py`

* `_scan(text, start, stops)` `[@ANCHOR: zero_sudo:css_scan]`: Finds where a CSS statement or declaration ends. `text` is the sheet (str), `start` the index to begin at (int, 0 to len), `stops` a string of stop characters. Returns the index of the first stop character at or after `start` that is outside a quoted string and outside parentheses, or `len(text)` when there is none; a string left open runs to the end. Pure, no errors. Used by `_parse_blocks` (stops `{;`) and `_clean_declarations` (stop `;`), so a `;` inside `url(a;b)` or `"a;b"` does not split a declaration. **Tests:** `TestCssSanitizerFunctions.test_01_scan_stops_only_outside_strings_and_parentheses`.
* `_matching_brace(text, start)` `[@ANCHOR: zero_sudo:css_matching_brace]`: Finds the `}` that closes the `{` at index `start` (int) of `text` (str), counting nested braces and ignoring braces inside quoted strings. Returns its index, or None when the block never closes. Pure. `_parse_blocks` calls it; an unclosed block makes the rest of the sheet a dropped fragment. **Tests:** `TestCssSanitizerFunctions.test_02_matching_brace_skips_nested_braces_and_strings`.
* `_parse_blocks(text)` `[@ANCHOR: zero_sudo:css_parse_blocks]`: Splits a sheet (or the inside of an `@media`/`@keyframes` block) into `[(prelude, body)]` items. `prelude` is the stripped text before `{` or `;`; `body` is the text between the braces, or None for a statement ended by `;` (an `@import ...;`), for an unclosed block, and for a trailing fragment, so those count as dropped by the caller. Pure; consumes the whole text and always terminates. **Tests:** `TestCssSanitizerFunctions.test_03_parse_blocks_splits_rules_and_statements`.
* `_clean_value(prop, raw)` `[@ANCHOR: zero_sudo:css_clean_value]`: Validates and rewrites one declaration value. `prop` is the lower-case property name (str), `raw` the value text. Returns the value with whitespace collapsed and any `!important` kept in canonical form, or None to drop the declaration. None is returned for an empty value, one over 500 characters, one matching the forbidden list (`javascript`, `expression`, `image-set(`, `data:`, backslash, `@`, `<`, braces, control characters and so on), unbalanced quotes or parentheses, any `url(` that is not `url(#fragment)`, and `position` other than `static`, `relative` or `absolute`. Pure. **Tests:** `TestCssSanitizerFunctions.test_04_clean_value_refuses_what_fetches_or_runs`.
* `_clean_declarations(body)` `[@ANCHOR: zero_sudo:css_clean_declarations]`: Cleans the inside of one `{...}` block. `body` is the declaration text. Returns `(text, dropped)`: the kept declarations as `prop:value` joined by `;`, and True when any declaration was removed. A declaration is dropped when it has no colon, its property is not a lower-case identifier (or is `behavior`, `-moz-binding`, `-ms-behavior`), its value fails `_clean_value`, or the rule already holds 200 declarations. Pure. **Tests:** `TestCssSanitizerFunctions.test_05_clean_declarations_keeps_the_good_ones`.
* `_clean_rules(text, depth, counter, in_keyframes=False)` `[@ANCHOR: zero_sudo:css_clean_rules]`: Rewrites the rules in `text`. `depth` is the `@media` nesting depth (0 at top level), `counter` a shared `_Counter` that counts rules across the whole sheet, `in_keyframes` is True inside `@keyframes`. Returns `(rules, dropped)`: the canonical text of accepted rules joined by newlines. Accepts style rules whose selector matches the selector pattern, `@media <plain condition>` up to depth 2 and top-level `@keyframes <name>`; every other at-rule, a statement without a block, and a selector with a brace is dropped. Stops at 2000 rules (returns what it has, dropped True). Recursive for nested blocks; pure apart from `counter`. **Tests:** `TestCssSanitizerFunctions.test_06_clean_rules_drops_what_is_not_allowed_and_keeps_the_rest`.
* `sanitize_stylesheet(text)` `[@ANCHOR: zero_sudo:css_sanitize_stylesheet]`: The entry point for the text of a member's `<style>` element. `text` is the author's CSS (str). Returns `(css, dropped)`: a stylesheet re-written only from the parts the rules accept, and True when anything other than a comment was removed. Blank input gives `("", False)`; input over 64 KiB, or containing a control character, gives `("", True)`. Comments are deleted (not replaced by a space), an unterminated comment sets `dropped`. Running it on its own output changes nothing. Pure, no I/O. **Tests:** `TestCssSanitizerFunctions.test_07_sanitize_stylesheet_rewrites_the_sheet`.
#### `svg_sanitizer.py`

* `_CallTotals.__init__(self)` `[@ANCHOR: zero_sudo:svg_call_totals_init]`: Creates the running totals for one `html_sanitize()` call or one `sanitize_xml_svgs` run: `blocks` (svg blocks seen so far) and `size` (bytes spent across all of them), both 0. The per-block caps alone do not bound many small blocks; the totals do (1000 blocks, 1 MiB). Shared by every `_Budget` of the call; not thread-safe, one call owns one instance. **Tests:** `TestSvgSanitizerFunctions.test_01_call_totals_and_budget_start_at_zero`.
* `_Budget.__init__(self, totals=None)` `[@ANCHOR: zero_sudo:svg_budget_init]`: Creates the work budget for one svg block. `totals` is the call's optional `_CallTotals` (borrowed, mutated by `spend`). `elements` and `size` start at 0. One `_Budget` per block, owned by `sanitize_svg_element`. **Tests:** `TestSvgSanitizerFunctions.test_01_call_totals_and_budget_start_at_zero`.
* `_Budget.element(self)` `[@ANCHOR: zero_sudo:svg_budget_element]`: Counts one more element in the block. Raises `_Rejected("too many elements")` once the count passes `MAX_SVG_ELEMENTS` (5000); the caller drops the whole block. Returns None. **Tests:** `TestSvgSanitizerFunctions.test_02_budget_element_counts_up_to_the_cap`.
* `_Budget.spend(self, amount)` `[@ANCHOR: zero_sudo:svg_budget_spend]`: Charges `amount` bytes (int) of output text or attribute data to the block and, when a `_CallTotals` was given, to the call. Raises `_Rejected` when the block passes `MAX_SVG_BYTES` (256 KiB) ("block too large") or the call passes `MAX_TOTAL_SVG_BYTES` (1 MiB) ("too much SVG in one value"). Returns None. **Tests:** `TestSvgSanitizerFunctions.test_03_budget_spend_caps_the_block_and_the_call`.
* `_check_enum(name)` `[@ANCHOR: zero_sudo:svg_check_enum]`: Builds the validator for an enumerated attribute. `name` is a key of `_ENUMS` (KeyError otherwise). The validator takes a value (str) and returns it unchanged when it is one of the allowed keywords (exact case), else None. Runs at import time to fill `_VALIDATORS`. **Tests:** `TestSvgSanitizerFunctions.test_04_check_enum_and_check_regex_build_validators`.
* `_check_regex(regex, limit=SCALAR_LIMIT)` `[@ANCHOR: zero_sudo:svg_check_regex]`: Builds the validator for a pattern-checked attribute. `regex` is a compiled pattern, `limit` the longest accepted value in characters (default 120; lists, paths and transforms pass a larger one so the patterns stay linear). The validator strips the value and returns it when it is within `limit` and matches `regex` completely, else None. The length check comes first so a pathological long value never reaches the regular expression. **Tests:** `TestSvgSanitizerFunctions.test_04_check_enum_and_check_regex_build_validators`.
* `_paint(value)` `[@ANCHOR: zero_sudo:svg_paint]`: Validator for `fill`, `stroke`, `stop-color` and `color`. Accepts a colour (name, `#rgb`, `rgb()`, `hsl()` and so on, `none`, `currentColor`) or `url(#id)` with an optional fallback colour. Returns the canonical text (`url(#id) fallback`), or None for anything else (an external `url(...)`, over 240 characters). Only a same-document fragment survives. **Tests:** `TestSvgSanitizerFunctions.test_05_paint_and_url_ref_keep_only_same_document_fragments`.
* `_url_ref(value)` `[@ANCHOR: zero_sudo:svg_url_ref]`: Validator for `clip-path` and `marker-*`. Accepts `none` or `url(#id)` (id per the id pattern) and returns `none` or the canonical `url(#id)`; returns None otherwise or for a value over 240 characters. **Tests:** `TestSvgSanitizerFunctions.test_05_paint_and_url_ref_keep_only_same_document_fragments`.
* `_viewbox(value)` `[@ANCHOR: zero_sudo:svg_viewbox]`: Validator for `viewBox`. Accepts exactly four numbers separated by spaces or commas and returns them joined by single spaces; None for any other count, a non-number, or over 240 characters. **Tests:** `TestSvgSanitizerFunctions.test_06_viewbox_needs_four_numbers`.
* `_safe_id(value)` `[@ANCHOR: zero_sudo:svg_safe_id]`: Validator for `id`. Accepts an identifier (letter first, up to 128 letters, digits, `_-.:`) that is not `__`-prefixed and not in `_CLOBBERING_IDS` (names such as `cookie`, `location`, `window` that would shadow a document property through named access, compared case-insensitively). Returns the stripped id or None. Stops DOM clobbering by an author's id. **Tests:** `TestSvgSanitizerFunctions.test_07_safe_id_refuses_clobbering_names`.
* `_aria(value)` `[@ANCHOR: zero_sudo:svg_aria]`: Validator for the kept aria attributes (`aria-label`, `aria-hidden`, `aria-roledescription`). Returns the value unchanged when it is at most 200 characters with no control character, `<`, `>`, quote, `&`, backslash or backtick, else None. **Tests:** `TestSvgSanitizerFunctions.test_08_aria_value_refuses_markup_characters`.
* `_small_margin(value)` `[@ANCHOR: zero_sudo:svg_small_margin]`: Validator for `margin-top` and `margin-bottom` in a `style`. Accepts a non-negative number of at most 99 with no unit or `px`, or at most 9 with `em` or `rem`, and returns it stripped; None otherwise (negative, larger, other units). Spacing only, never overlap. **Tests:** `TestSvgSanitizerFunctions.test_09_small_margin_and_svg_size_have_caps`.
* `_svg_size(value)` `[@ANCHOR: zero_sudo:svg_size]`: Bounds `width` and `height` on the `<svg>` element itself. Accepts `auto`, or a number (up to 3 decimals) with no unit or `px` up to 4000, `%` up to 100, `em`/`rem` up to 250; returns the stripped value, else None. An svg paints above neighbouring text, so an oversized one would cover links. **Tests:** `TestSvgSanitizerFunctions.test_09_small_margin_and_svg_size_have_caps`.
* `_clean_style(value)` `[@ANCHOR: zero_sudo:svg_clean_style]`: Rebuilds a `style` attribute value (str). Returns `prop:value;...` containing only declarations whose property is in `_STYLE_PROPS` and whose value passes that property's own validator and the bad-value filter, or None when none remain, when the value is over 2000 characters, or when it contains `/*`, `@` or a backslash. **Tests:** `TestSvgSanitizerFunctions.test_10_clean_style_keeps_allowlisted_declarations`.
* `_clean_attribute(name, value)` `[@ANCHOR: zero_sudo:svg_clean_attribute]`: Validates one attribute of an svg element. `name` and `value` are str (anything else gives None). Returns `(canonical name, cleaned value)` or None. `aria-*` is kept only for the three allowed names; `style` goes through `_clean_style`; everything else must be in `ALLOWED_ATTRIBUTES` (matched case-insensitively and returned in canonical case, e.g. `viewBox`), at most 20000 characters, pass its validator and the bad-value filter. Event handlers, `href`, `class`, `data-*` are not in the list and so are dropped. **Tests:** `TestSvgSanitizerFunctions.test_11_clean_attribute_returns_canonical_name_and_value`.
* `_clean_text(text, keeps_text, budget)` `[@ANCHOR: zero_sudo:svg_clean_text]`: Cleans one text or tail string. `keeps_text` is True inside `text`, `tspan`, `title` and `desc`, where character data is rendered: control characters are removed and the length charged to `budget`. Elsewhere only whitespace survives and is reduced to a single newline. Returns the text or None for nothing. **Tests:** `TestSvgSanitizerFunctions.test_12_clean_text_and_join_text_normalise_whitespace`.
* `_join_text(first, second, keeps_text)` `[@ANCHOR: zero_sudo:svg_join_text]`: Concatenates two text pieces (`first` may be None) after a dropped child element. Where text is not rendered (`keeps_text` False), a whitespace-only result becomes one newline so a second sanitising pass changes nothing. **Tests:** `TestSvgSanitizerFunctions.test_12_clean_text_and_join_text_normalise_whitespace`.
* `svg_local_name(tag)` `[@ANCHOR: zero_sudo:svg_local_name]`: Gives the lower-case local name of an SVG-vocabulary tag, or None for a foreign one. An HTML-parsed tree has bare names (`rect`), an XML-parsed QWeb tree has Clark names (`{http://www.w3.org/2000/svg}rect`); both are accepted. A tag in any other namespace, with an undeclared prefix (`svg:script`), or that is not a str (a comment) gives None. **Tests:** `TestSvgSanitizerFunctions.test_13_svg_local_name_accepts_bare_and_svg_namespace_names`.
* `_build(source, depth, budget, dropped_attributes=())` `[@ANCHOR: zero_sudo:svg_build]`: Rebuilds one source element as a clean new element, recursively. `source` is an lxml element, `depth` its nesting level (1 for the svg), `budget` the `_Budget`, `dropped_attributes` canonical names the calling field refuses (`style`, `class`). Returns the new element, or None when the tag is not an allowlisted element (its tail text is kept by the parent). Only allowlisted attributes with validated values are copied; on an `<svg>`, `transform` is dropped and `width`/`height` bounded by `_svg_size`. Raises `_Rejected` for depth over 32, over the element count or over a size cap. Nothing the author wrote is copied except through a validator. **Tests:** `TestSvgSanitizerFunctions.test_14_build_and_sanitize_element_rebuild_from_the_allowlist`.
* `sanitize_svg_element(source, dropped_attributes=(), _totals=None)` `[@ANCHOR: zero_sudo:svg_sanitize_element]`: Public entry for one `<svg>` block. `source` is an lxml element, `dropped_attributes` as for `_build`, `_totals` the call's `_CallTotals`. Returns a rebuilt copy with `xmlns` set, or None when the element is not an svg or any cap is passed (block size, 5000 elements, depth 32, 1000 blocks or 1 MiB for the call); a cap logs a warning and the block is dropped whole, never truncated. **Tests:** `TestSvgSanitizerFunctions.test_14_build_and_sanitize_element_rebuild_from_the_allowlist`.
* `_top_level_svgs(doc)` `[@ANCHOR: zero_sudo:svg_top_level_svgs]`: Lists the `svg` elements of an HTML tree that have no `svg` ancestor, in document order. A nested svg is handled by its outer block's rebuild. **Tests:** `TestSvgSanitizerFunctions.test_15_detach_top_level_svgs_and_reattach_them`.
* `_detach_svgs(doc, nonce, dropped_attributes)` `[@ANCHOR: zero_sudo:svg_detach_svgs]`: Step 1 of `clean_document`. Rebuilds every top-level svg of `doc` (an lxml tree, mutated) and replaces each with a placeholder comment `hamssvg<nonce>x<index>x` (`nonce` is a random hex string); a comment so the swap is O(1) per block and the placeholder cannot land in an attribute. A document that is itself one svg becomes a `div` holding the placeholder. Returns the stash `{index: rebuilt element or None}`; None entries are blocks past a cap. **Tests:** `TestSvgSanitizerFunctions.test_15_detach_top_level_svgs_and_reattach_them`.
* `_in_blocked_context(element)` `[@ANCHOR: zero_sudo:svg_in_blocked_context]`: True when `element` or any ancestor is one whose content a browser reads as raw text or another language (`textarea`, `title`, `style`, `script`, `math`, `svg`, `iframe`, `template`, ...). A rebuilt svg is never put back there. **Tests:** `TestSvgSanitizerFunctions.test_16_a_block_in_a_blocked_context_is_removed_with_its_tails_kept`.
* `_reattach_svgs(doc, stash, nonce)` `[@ANCHOR: zero_sudo:svg_reattach_svgs]`: Step 3 of `clean_document`. For each surviving placeholder comment carrying this call's `nonce`, puts the stashed svg back in its place (keeping the comment's tail), unless the block was dropped or its parent is in a blocked context. Every placeholder is removed either way, with its tail text merged into the neighbour by `_remove_keeping_tails`. A forged placeholder with another nonce is left to the stock cleaner. **Tests:** `TestSvgSanitizerFunctions.test_15_detach_top_level_svgs_and_reattach_them`.
* `_remove_keeping_tails(parent, comments)` `[@ANCHOR: zero_sudo:svg_remove_keeping_tails]`: Removes the placeholder `comments` (a set of comment elements) that are children of `parent`, in one pass, appending each removed comment's tail to the preceding text. Linear: merging one tail at a time with `+=` was quadratic for thousands of adjacent dropped blocks. **Tests:** `TestSvgSanitizerFunctions.test_17_remove_keeping_tails_and_store_text_merge_tail_text`.
* `_store_text(parent, holder, pieces)` `[@ANCHOR: zero_sudo:svg_store_text]`: Writes the joined `pieces` (list of str) as the text of `parent` when `holder` is None, else as the tail of `holder` (an lxml element); an empty result is stored as None. Helper of `_remove_keeping_tails`. **Tests:** `TestSvgSanitizerFunctions.test_17_remove_keeping_tails_and_store_text_merge_tail_text`.
* `clean_document(doc, run_cleaner, dropped_attributes=())` `[@ANCHOR: zero_sudo:svg_clean_document]`: Runs `run_cleaner(doc)` (a one-argument callable: the stock Odoo cleaner) with every svg block validated around it. With no `<svg>` in `doc` it simply returns `run_cleaner(doc)`. Otherwise it detaches the blocks (`_detach_svgs`), runs the cleaner on the rest, puts the surviving rebuilt blocks back (`_reattach_svgs`) and returns the cleaner's result. `dropped_attributes` as for `_build`. The nonce is `secrets.token_hex(12)`, so author text cannot forge a placeholder. Called from the wrapper `install()` puts on `_Cleaner.__call__`. **Tests:** `TestSvgSanitizerFunctions.test_15_detach_top_level_svgs_and_reattach_them`.
* `_looks_hostile(source)` `[@ANCHOR: zero_sudo:svg_looks_hostile]`: Decides whether an svg block is an attack rather than editor leftovers. True when any element is one of `script`, `foreignObject`, `use`, `animate*`, `set`, `style`, `iframe`, `image` and similar, when any attribute is an `on...` handler, or when a URL-carrying attribute (`href`, `src`, `values`, `style`, ...) holds a `javascript:`, `vbscript:` or `data:` scheme once whitespace and control characters are removed. Only used to decide whether the author is struck; the block is rebuilt either way. **Tests:** `TestSvgSanitizerFunctions.test_18_looks_hostile_tells_attacks_from_editor_leftovers`.
* `_drop_keeping_tail(element)` `[@ANCHOR: zero_sudo:svg_drop_keeping_tail]`: Removes `element` from its parent, first giving its tail text to the previous sibling's tail or, when it is first, to the parent's text. Does nothing for an element with no parent. Used by `sanitize_xml_svgs` for a block that failed validation. **Tests:** `TestSvgSanitizerFunctions.test_19_drop_keeping_tail_hands_the_tail_to_the_neighbour`.
* `sanitize_xml_svgs(root)` `[@ANCHOR: zero_sudo:svg_sanitize_xml_svgs]`: For XML-parsed trees (QWeb arch), where the HTML hook never runs. `root` is an lxml tree, mutated in place. Every outermost element whose local name is `svg` in any namespace or with an undeclared prefix is replaced by its `sanitize_svg_element` rebuild (every `t-*` attribute inside it is dropped), or removed (tail kept) when it cannot be validated. Returns True when something hostile was found (a `_looks_hostile` block, thousands of blocks, or a foreign-namespace svg) so the caller can strike the author; harmless drops return False. **Tests:** `TestSvgSanitizerFunctions.test_20_sanitize_xml_svgs_rebuilds_blocks_and_reports_hostility`.
* `install()` `[@ANCHOR: zero_sudo:svg_install]`: Makes Odoo's HTML sanitizer SVG-aware by replacing `odoo.tools.mail._Cleaner.__call__` with a wrapper that calls `clean_document` (dropping `style` when the cleaner strips styles, `class` when it strips classes). Takes no arguments, returns None, idempotent: a second call finds the wrapper's `_hams_svg_allowlist` marker and does nothing. Called once from `zero_sudo/__init__.py` at import time; global, process-wide effect. **Tests:** `TestSvgSanitizerFunctions.test_21_install_is_idempotent`.

### `zero_sudo.daemon.utils`
Provides centralized, private utilities for managing background daemon processes tightly coupled with Odoo.
* `_start_daemon_process(script_path, args, env_vars)` `[@ANCHOR: zero_sudo:_start_daemon_process]`: Starts `script_path` under the current Python interpreter (`sys.executable`, falling back to `/usr/bin/python3`) with `shell=False`, in its own session so it is detached from the web request and so `_stop_daemon_process` can signal its whole process group. The child's `PYTHONPATH` is the parent's `sys.path` followed by any `PYTHONPATH` already in the environment, which lets a daemon import Odoo addons; `env_vars` are then merged over that environment. It returns the `Popen` object.
* `_stop_daemon_process(process)`: Safely terminates a running daemon.
* `_poll_health_check(url, timeout, interval)` `[@ANCHOR: zero_sudo:_poll_health_check]`: Polls an HTTP endpoint to verify daemon health before proceeding with tests. It sends `HEAD` requests until one returns 200, accepts only `http` and `https` URLs, and raises `UserError` after `timeout` seconds (default 30).

### Shared offline queue (`static/src/js/offline_store.js`)
`OfflineStore` is a small IndexedDB-backed queue that frontend modules (the QSO (radio contact) logger in `ham_shack`, `ics_forms`) use to keep a record locally while the browser is offline and send it later. It keeps records in an object store keyed by `uuid` with an index on `timestamp`; the database and store names are constructor arguments whose defaults match the names `ham_shack` first used, so existing browser data survived the move into this module. Its methods are `init()`, `saveLog(logData)`, `getPendingLogs()` and `deleteLog(uuid)`.

`isStorageFullError(err)` `[@ANCHOR: is_storage_full_error]` tells a caller whether a rejected `saveLog()` means the browser refused the write because the disk or the site's storage quota is full. It returns true for a `QuotaExceededError` (code 22) as Chrome and the spec raise it, and for Firefox's `NS_ERROR_DOM_QUOTA_REACHED` (code 1014), and false for anything else, including no error at all. Callers use it to tell the operator "Disk Full" instead of a generic or misleading network error.

### Tour watchdog and its teardown gate (`static/src/js/tour_failure_dump.js`, `static/src/js/tour_abort_gate.js`)
`tour_failure_dump.js` is loaded in the test-only `web.assets_tests` bundle. It turns an uncaught JS error, an unhandled promise rejection, a dropped XHR or the browser going offline into a "[WATCHDOG ALARM] FATAL JS EVENT DETECTED" dump (the error, the page phase, a stack and a compact DOM skeleton) written with `console.error`. Odoo's headless browser (`odoo/tests/common.py`, `ChromeBrowser._handle_console`) records every `console.error` as `had_failure` before it checks whether the tour is already over, and `_wait_code_ok` then fails the test even though the tour printed "tour succeeded". So an alarm raised after success failed whole runs on its own: Odoo's lazily loaded portal chatter stylesheet (`loadCssFromBundle` in `mail`, which rejects with the link's `error` Event) is still in flight when a `/my/ticket/<id>` tour ends and Python closes Chrome (seen on every `hams_helpdesk` run of the slower test host, 68 ms after success), and a last step that makes the page reload makes the tour engine's `beforeunload` check throw after the tour is gone (`binary_downloader`). The pure helpers sit in `tour_abort_gate.js`, which installs nothing, so a unit test can import it.

* `describeRejectionReason(reason)` `[@ANCHOR: zero_sudo:js_describe_rejection_reason]`: for a rejection whose reason is a DOM `Event` it returns `"[object Event] "` followed by the event's type, constructor and, when it still has a target, the target's tag, URL, `readyState` and error (or `target=none` when the target is gone, as with a shadow-root link or a closing page); any other reason is returned unchanged. Cited by `tour_abort_gate.test.js` and `tests/test_tour_abort_gate.py`.
* `describePhase(phase, now, doc)` `[@ANCHOR: zero_sudo:js_describe_phase]`: one line, `Phase: tour succeeded <n> ms ago|no; unloading <n> ms ago|no; readyState=...; visibility=...`, from `phase.succeededAt` and `phase.unloadingAt` (milliseconds since the epoch, 0 for not yet), `now` (`Date.now()`) and the document. Every alarm carries it. `window._hamsPhase` is the live `phase`: `succeededAt` is set when the tour engine logs exactly `tour succeeded`, `unloadingAt` at the first `beforeunload` or `pagehide`.
* `abortChannel(phase)` `[@ANCHOR: zero_sudo:js_abort_channel]`: returns `"fatal"` until `phase.succeededAt` is set and `"teardown"` afterwards (also `"fatal"` for no phase). A page unload on its own never makes it `"teardown"`, since an unload during a step with `expectUnloadPage` is part of the tour. `"fatal"` keeps the `console.error` alarm, so any error inside a tour still fails the test. `"teardown"` writes the same dump with `console.warn` under the heading `[WATCHDOG NOTICE] JS event after the tour succeeded, while its page was torn down (not a failure)`, which Odoo logs as a WARNING; `tools/test.py` does not match that heading.

### `zero_sudo.kv`
A high-performance Key-Value store table with cached reads (`_get_kv()` is `@distributed_cache`-decorated) and atomic writes via PostgreSQL procedures. In this repository it is used by the documentation bootstrap to record each installed document's content hash and article ID.
* Developers interact with this model via `_get_kv()` and `_set_kv()` in `zero_sudo.security.utils`.

### `zero_sudo.noisy_table`
A registry model (`zero_sudo_noisy_table`) used to declare PostgreSQL tables (by table name) that undergo high-frequency or technically necessary data mutations (e.g. `ir_logging`, `bus_bus`). Tables registered here are exempt from the strict end-of-test Database Leak Prevention checks of `RealTransactionCase` (`tests/real_transaction.py`), which also always skips its own built-in list of such tables.

### `zero_sudo.security.log`
The centralized audit log model (`zero_sudo_security_log`). Records blocked service-account logins, denied system-parameter reads and writes, and cache invalidation commands. 
* Rows are retained forever: no scheduled job purges, expires or truncates this log.
* Immutable design ensures even `base.group_system` users cannot delete or modify log records via the UI or ORM.

---

## 6. External Dependencies
* `python`: `["markdown", "psycopg2", "requests"]`
