# Zero-Sudo Security Core [@ANCHOR: zero_sudo_main] (`zero_sudo`)

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).*

This module is the foundational security layer for our Odoo ecosystem. It enforces a strict **Zero-Sudo Architecture** (ADR-0002) to prevent privilege escalation vulnerabilities and physically isolates background service accounts from interactive web sessions (ADR-0005).

## 🛡️ Core Security Missions

1.  **Eliminate `.sudo()` Usage:** Developers are forbidden from using Odoo's native `.sudo()` command. This module provides a secure, audited alternative via the **Service Account Pattern**.
2.  **Web Isolation:** Automated background tasks (Service Accounts) are strictly prohibited from logging into the website interface, preventing session hijacking or accidental human interference.
3.  **Mechanical Whitelisting:** Critical system parameters (like cryptographic keys) are blocked from being accessed or modified through the UI unless they are explicitly registered in a secure code-level whitelist.
4.  **Security Audit Trail:** All blocked login attempts and security events are logged for administrator review.
5.  **High-Performance Security Interop:** Critical security lookups and atomic KV operations are optimized via PostgreSQL procedures to minimize latency and ensure transaction-safe execution.

---

## 📖 User Guide (Non-Technical)

### What is a Service Account?
A "Service Account" is a special user profile used only by background programs, bots, and automated tasks. Because these accounts often have powerful permissions, they are **forbidden** from being used to log into the website through a browser.

### Managing Service Accounts:
1.  **Restricting an Account:** Go to **Settings > Users**, select a user, and check the **"Is Service Account"** box.
2.  **Access Denied:** Once flagged, any login attempt with that account via the browser will result in an "Access Denied" error. This is a deliberate security feature.
3.  **Automatic Protection:** Every Service Account is automatically assigned an extremely long, randomized password that is impossible to guess, ensuring it can only be accessed by the automated system intended to use it.

### Security Audit Logs:
Administrators can monitor security events in **Settings > Security Logs**. The following events are tracked:
*   **Service Account Web Login Attempt:** Logs when a background account tries to access the UI.
*   **Privilege Escalation Security Block Tripped:** Logs when an automated account attempts to escalate to global administrative privileges.
*   **Unauthorized System Parameter Access:** Logs when a process attempts to read or write a restricted system setting.
*   **Model Cache Invalidation:** Logs when a user or process manually triggers a system-wide cache refresh.

---

# Technical Documentation

<system_role>
**Context:** Technical documentation strictly for LLMs and Integrators developing custom downstream modules...
</system_role>

<architecture>
## Core Architecture

1.  **Service Account Pattern**: High-privilege operations are offloaded to dedicated `res.users` records flagged with `is_service_account=True`.
2.  **Centralized Security Utilities**: The `zero_sudo.security.utils` model provides cached methods for secure UID retrieval, system parameter whitelisting, and deterministic hashing.
3.  **Web Isolation Interceptor**: A controller override on `web_login` that uses raw SQL to check and block interactive logins for service accounts.
4.  **Automated Documentation Bootstrap**: An inheritance of `ir.module.module` that centrally manages the installation of HTML documentation from module manifests.
</architecture>

<security_design>
## Security Design (ADR-0002, ADR-0005)

-   **Anti-IDOR & Privilege Escalation**: `_get_service_uid` performing direct SQL lookups strictly rejects any service account with global administrative groups (`base.group_system`, `base.group_erp_manager`).
-   **Mechanical Secret Block**: `_get_system_param` and `_set_system_param` enforce a hardcoded `PARAM_WHITELIST` and block keys matching cryptographic patterns (e.g., "secret", "token") to prevent SSTI exfiltration.
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

When a daemon strictly requires native ERP framework interactions that mandate `base.group_user`, they MUST temporarily assume one of the two centralized proxy accounts:

### A. Central Mail Service Account (`zero_sudo.mail_service_internal`)
* **Use Case:** Execute `message_post()`, `send_mail()`, or interact with the `mail.thread` chatter.

### B. Odoo Facility Service Account (`zero_sudo.odoo_facility_service_internal`)
* **Use Case:** Complex ORM cascades that deeply assume internal user rights.
</shared_service_accounts>

---

<python_api>
## 3. Python API Reference (`zero_sudo.security.utils`)

#### `_get_service_uid(xml_id)` `[@ANCHOR: get_service_uid]`
Safely retrieves the database ID of a Service Account. Result is RAM-cached.
* **Arguments:** `xml_id` (str): The external ID (e.g., `'your_module.your_service_account'`).
* **Returns:** `int` (The User ID).

#### `_get_deterministic_hash(input_string)` `[@ANCHOR: deterministic_hash]`
Generates a deterministic 32-bit integer hash for `pg_advisory_xact_lock`.
* **Returns:** `int`.

#### `_get_system_param(key, default=None)` `[@ANCHOR: get_system_param]`
Safely retrieves a whitelisted system configuration parameter.

#### `_notify_cache_invalidation(model_name, key_value)` `[@ANCHOR: coherent_cache_signal]`
Emits a PostgreSQL `NOTIFY` event to synchronize distributed caches.

#### `_get_crypto_secret()` `[@ANCHOR: get_crypto_secret]`
Retrieves the root cryptographic key from environment or local file, bypassing DB.

#### `_invalidate_model_cache(model_name)` `[@ANCHOR: invalidate_model_cache]`
Securely invalidates the entire cache for a specific model. Requires the user to have write access to the model. Events are audited.

#### `_set_kv(key, value)` `[@ANCHOR: set_kv_procedure]`
High-performance atomic key-value update using an optimized Postgres procedure.
</python_api>

---

<tour_utils>
## 4. UI Tour Macros (`tour_utils.js`)

The `TourUtils` object (`@zero_sudo/js/tour_utils`) provides centralized macros for Odoo UI Tours to guarantee architectural compliance and eliminate race conditions in headless browser testing.

#### `safeSave(saveButtonTrigger, waitTrigger)`
Generates a macro array to safely click a save button and explicitly wait for the subsequent RPC resolution or UI state change.

#### `bypassDialogs()`
Generates a macro to intercept and bypass native blocking dialogs (`window.alert`, `window.confirm`) during testing.

#### `mockExternalRequests(urlPattern, mockResponse)`
Generates a macro to intercept and mock `fetch` requests matching a specific URL pattern, preventing external network calls during tests.

#### `waitForAbsence(selector, description)`
Generates a macro that polls and explicitly waits for a specific DOM element to disappear from the page.

#### `deterministicInput(helpers, text)`
A direct execution function (used within a tour's `run` step) that safely injects text into the currently active input element and explicitly dispatches the `input`, `change`, and `keyup` events required to reliably awaken Odoo's frontend framework (e.g., Many2one debouncers).
</tour_utils>

---

## 6. External Dependencies
* `python`: `["psycopg2", "requests"]`
