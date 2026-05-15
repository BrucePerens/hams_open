# LLM Documentation: Zero-Sudo Security Core (`zero_sudo`)

<system_role>
This module is the foundational security layer for the ecosystem. It enforces a strict **Zero-Sudo Architecture** and **Service Account Web Isolation**. It provides the mechanical enforcement of least-privilege principles by replacing Odoo's dangerous `.sudo()` method with micro-privilege service accounts and centralized security utilities.
</system_role>

<architecture>
## Core Architecture

The module utilizes several key patterns to secure the system:
1.  **Service Account Pattern**: High-privilege operations are offloaded to dedicated `res.users` records flagged with `is_service_account=True`.
2.  **Centralized Security Utilities**: The `zero_sudo.security.utils` model provides cached methods for secure UID retrieval, system parameter whitelisting, and deterministic hashing.
3.  **Web Isolation Interceptor**: A controller override on `web_login` that uses raw SQL to check and block interactive logins for service accounts.
4.  **Automated Documentation Bootstrap**: An inheritance of `ir.module.module` that centrally manages the installation of HTML documentation from module manifests.
</architecture>

<security_design>
## Security Design (ADR-0002, ADR-0005)

-   **Anti-IDOR & Privilege Escalation**: The `_get_service_uid` method performs direct SQL lookups and strictly rejects any service account with global administrative groups (`base.group_system`, `base.group_erp_manager`).
-   **Mechanical Secret Block**: `_get_system_param` and `_set_system_param` enforce a hardcoded `PARAM_WHITELIST` and block keys matching cryptographic patterns (e.g., "secret", "token") to prevent SSTI exfiltration.
-   **Coherent Cache Signaling**: Uses PostgreSQL `NOTIFY` (`pg_notify`) to synchronize in-memory caches across distributed worker nodes.
</security_design>

<stories_and_journeys>
## Stories and Journeys

### Stories
-   **Secure Privilege Escalation** [`@ANCHOR: story_secure_escalation`]: zero_sudo/docs/stories/secure_escalation.md
-   **Blocking Service Account Login** [`@ANCHOR: story_login_blocking`]: zero_sudo/docs/stories/login_blocking.md
-   **Parameter Whitelisting** [`@ANCHOR: story_parameter_whitelisting`]: zero_sudo/docs/stories/parameter_whitelisting.md
-   **Coherent Cache Signaling** [`@ANCHOR: story_cache_signaling`]: zero_sudo/docs/stories/cache_signaling.md
-   **Deterministic Hashing** [`@ANCHOR: story_deterministic_hash`]: zero_sudo/docs/stories/deterministic_hashing.md
-   **Python VENV Management** [`@ANCHOR: story_venv_management`]: zero_sudo/docs/stories/venv_management.md
-   **Centralized Documentation Bootstrap** [`@ANCHOR: story_zero_sudo_doc_installer`]: zero_sudo/docs/stories/documentation_bootstrap.md

### Journeys
-   **Service Account Lifecycle** [`@ANCHOR: journey_service_account_lifecycle`]: zero_sudo/docs/journeys/service_account_lifecycle.md
-   **Securing Configuration Parameters** [`@ANCHOR: journey_securing_configuration`]: zero_sudo/docs/journeys/securing_configuration.md
</stories_and_journeys>

<api_contracts>
## API Contracts

### `zero_sudo.security.utils`
-   `_get_service_uid(xml_id)` [`@ANCHOR: get_service_uid`]: Returns user ID for a valid Service Account.
-   `_get_system_param(key, default=None)` [`@ANCHOR: get_system_param`]: Securely retrieves a whitelisted system parameter.
-   `_get_deterministic_hash(input_string)` [`@ANCHOR: deterministic_hash`]: Generates stable integer hashes for Postgres advisory locks.
-   `_notify_cache_invalidation(model_name, key_value)` [`@ANCHOR: coherent_cache_signal`]: Emits cross-worker invalidation signal.
</api_contracts>
