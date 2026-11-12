# Data structures

### `TEscWarningFilter` [@ANCHOR: zero_sudo:zero_sudo___init___tesc_warning_filter]

**Purpose and location.** Suppresses noisy Odoo 15+ QWeb deprecation warnings for @t-esc to prevent polluting test logs. Defined in `zero_sudo/__init__.py:24` as a class; bases: `logging.Filter`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `filter`.

**See also:** test `test_29_preload_service_uid_cache_populates_known_service_accounts_only (tests/test_security_utils.py)`.

### `ZeroSudoHome` [@ANCHOR: zero_sudo:zero_sudo_main_zero_sudo_home]

**Purpose and location.** The class has no docstring; from its definition it provides `web_login`. Defined in `zero_sudo/controllers/main.py:14` as a class; bases: `Home`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `web_login`.

**See also:** test `test_01_web_login_interceptor (tests/test_controllers.py)`.

### `ZeroSudoSession` [@ANCHOR: zero_sudo:zero_sudo_main_zero_sudo_session]

**Purpose and location.** The class has no docstring; from its definition it provides `authenticate`. Defined in `zero_sudo/controllers/main.py:86` as a class; bases: `Session`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `authenticate`.

**See also:** test `test_debug_assets_serves_unminified_web_bundle (tests/test_debug_assets_propagation.py)`.

### `_Counter` [@ANCHOR: zero_sudo:zero_sudo_css_sanitizer_counter]

**Purpose and location.** The class has no docstring; from its definition it provides class attributes only. Defined in `zero_sudo/css_sanitizer.py:183` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_06_clean_rules_drops_what_is_not_allowed_and_keeps_the_rest (tests/test_sanitizer_functions.py)`.

### `SpamConfig` [@ANCHOR: zero_sudo:zero_sudo_club_spam_spam_config]

**Purpose and location.** Every tunable, with its default. The Odoo system parameter for a field is PARAM_PREFIX + the field name. Defined in `zero_sudo/daemon/club_spam.py:96` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `from_params`.

**See also:** test `test_33_the_persistence_threshold_is_a_table (tests/test_club_spam.py)`.

### `EntryScore` [@ANCHOR: zero_sudo:zero_sudo_club_spam_entry_score]

**Purpose and location.** The class has no docstring; from its definition it provides class attributes only. Defined in `zero_sudo/daemon/club_spam.py:247` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_21_a_hidden_block_of_casino_links_is_found_and_css_classes_are_not_read (tests/test_club_spam.py)`.

### `_HomeParser` [@ANCHOR: zero_sudo:zero_sudo_club_spam_home_parser]

**Purpose and location.** Collects what the spam rules need from one home page. Not a tree: a stack of open tags with two inherited flags (hidden by an inline style, inside a recent-posts widget). Defined in `zero_sudo/daemon/club_spam.py:400` as a class; bases: `HTMLParser`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`, `handle_starttag`, `handle_endtag`, `handle_data`.

**See also:** test `test_20_the_recent_posts_widget_and_links_of_a_hacked_page_are_read (tests/test_club_spam.py)`.

### `_StripParser` [@ANCHOR: zero_sudo:zero_sudo_club_spam_strip_parser]

**Purpose and location.** Finds the character spans of a page that are spam: a recent-posts widget most of whose links are flagged, a hidden element holding a flagged link, a list item whose only link is flagged, a flagged link anywhere else. Defined in `zero_sudo/daemon/club_spam.py:904` as a class; bases: `HTMLParser`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`, `_offset`, `handle_starttag`, `handle_data`, `handle_endtag`.

**See also:** test `test_53_spam_blocks_are_cut_from_a_page_before_a_model_sees_it (tests/test_club_spam.py)`.

### `SecureJSONRPCClient` [@ANCHOR: zero_sudo:zero_sudo_json_rpc_client_secure_jsonrpcclient]

**Purpose and location.** Standardized JSON-2 IPC client for external daemons. Reads credentials strictly from local daemon_key_manager .env files. Implements self-healing retry logic upon key rotation. Defined in `zero_sudo/daemon/json_rpc_client.py:14` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`, `_load_credentials`, `call`.

**See also:** test `test_call_success (tests/test_json_rpc_client.py)`.

### `BrokerConfigError` [@ANCHOR: zero_sudo:zero_sudo_broker_broker_config_error]

**Purpose and location.** The broker configuration is unusable; the broker refuses to start. Defined in `zero_sudo/daemon/secret_broker/broker.py:66` as a class; bases: `ValueError`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_config_validation (tests/test_secret_broker.py)`.

### `Refused` [@ANCHOR: zero_sudo:zero_sudo_broker_refused]

**Purpose and location.** A request the broker will not serve; `code` is what the caller sees. Defined in `zero_sudo/daemon/secret_broker/broker.py:70` as a class; bases: `Exception`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`.

**See also:** test `test_paths (tests/test_secret_broker.py)`.

### `Broker` [@ANCHOR: zero_sudo:zero_sudo_broker_broker]

**Purpose and location.** The request handler: policy + secret source. `handle()` is the whole decision for one request. Defined in `zero_sudo/daemon/secret_broker/broker.py:297` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`, `check_secrets_present`, `handle`, `_run`, `serve_connection`, `serve`.

**See also:** test `test_06_get_deterministic_hash (tests/test_security_utils.py)`.

### `SecretStoreError` [@ANCHOR: zero_sudo:zero_sudo_secret_store_secret_store_error]

**Purpose and location.** Base class: the store could not give the caller what it asked for. Defined in `zero_sudo/daemon/secret_store.py:55` as a class; bases: `RuntimeError`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_wrong_peer_uid_is_refused_on_the_real_socket (tests/test_secret_broker.py)`.

### `SecretNotFound` [@ANCHOR: zero_sudo:zero_sudo_secret_store_secret_not_found]

**Purpose and location.** No configured backend has this secret. Defined in `zero_sudo/daemon/secret_store.py:59` as a class; bases: `SecretStoreError`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_on_missing_database_secret_is_loud (tests/test_core_secrets_from_store.py)`.

### `InsecureSecretSource` [@ANCHOR: zero_sudo:zero_sudo_secret_store_insecure_secret_source]

**Purpose and location.** A secret file has an owner or mode that would let the wrong account read or rewrite it. Defined in `zero_sudo/daemon/secret_store.py:63` as a class; bases: `SecretStoreError`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_private_file_rules (tests/test_secret_store.py)`.

### `SystemdCredentialBackend` [@ANCHOR: zero_sudo:zero_sudo_secret_store_systemd_credential_backend]

**Purpose and location.** Tier A: `$CREDENTIALS_DIRECTORY/<name>`, one file per secret, put there by systemd. Defined in `zero_sudo/daemon/secret_store.py:124` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`, `get`.

**See also:** test `test_wrong_peer_uid_is_refused_on_the_real_socket (tests/test_secret_broker.py)`.

### `CredentialsFileBackend` [@ANCHOR: zero_sudo:zero_sudo_secret_store_credentials_file_backend]

**Purpose and location.** Tier A: one root-owned `name=value` file, readable by the service account through its group. Defined in `zero_sudo/daemon/secret_store.py:147` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`, `get`.

**See also:** test `test_credentials_file_backend (tests/test_secret_store.py)`.

### `EncryptedBackend` [@ANCHOR: zero_sudo:zero_sudo_secret_store_encrypted_backend]

**Purpose and location.** Tier A fallback: AES-256-GCM values, key outside the database.  `key_loader()` returns the 32-byte key (for example from a systemd credential or a root-owned key file, see `key_from_file`); `lookup(name)` returns the stored token or None. The token is `hs1.` + urlsafe base64 of nonce (12 bytes) + ciphertext, with `_AAD_PREFIX + name` as associated data, made by `encrypt_value`. Defined in `zero_sudo/daemon/secret_store.py:166` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`, `get`.

**See also:** test `test_encrypted_backend_round_trip_and_binding (tests/test_secret_store.py)`.

### `BrokerClient` [@ANCHOR: zero_sudo:zero_sudo_secret_store_broker_client]

**Purpose and location.** Tier B client: one JSON request per connection to the broker's unix socket (secret_broker.py). The broker decides who may ask from SO_PEERCRED; the client sends no credential of its own. Defined in `zero_sudo/daemon/secret_store.py:247` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`, `call`.

**See also:** test `test_wrong_peer_uid_is_refused_on_the_real_socket (tests/test_secret_broker.py)`.

### `SecretStore` [@ANCHOR: zero_sudo:zero_sudo_secret_store_secret_store]

**Purpose and location.** Ordered Tier A backends plus an optional Tier B broker.  `get(name)` hands out a secret (Tier A only; the broker never does). The operation methods (`hmac_sign`, `hmac_verify`, `stripe_webhook_verify`) go to the broker when there is one and are otherwise computed here from `get(name)`, with the same code (secret_operations.py). Defined in `zero_sudo/daemon/secret_store.py:277` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`, `get`, `require`, `hmac_sign`, `hmac_verify`, `stripe_webhook_verify`, `https_request`.

**See also:** test `test_a_secret_that_has_gone_or_a_nested_request_gets_an_error_reply (tests/test_secret_broker.py)`.

### `SSRFValidationError` [@ANCHOR: zero_sudo:zero_sudo_ssrf_safe_fetch_ssrfvalidation_error]

**Purpose and location.** Raised when a URL's hostname is missing, unresolvable, or resolves to a non-public address. Deliberately a plain, Odoo-free exception type -- each caller translates it into whatever fits its own context (`UserError` in `binary_utils.py`, `ValueError` in `pager_synthetic_spooler.py`) rather than this shared module picking one Odoo-flavored type for every caller. Defined in `zero_sudo/daemon/ssrf_safe_fetch.py:71` as a class; bases: `Exception`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_resolve_ssrf_safe_addresses_raises_for_a_missing_hostname (tests/test_ssrf_safe_fetch.py)`.

### `_SSRFSafeHTTPHandler` [@ANCHOR: zero_sudo:zero_sudo_ssrf_safe_fetch_ssrfsafe_httphandler]

**Purpose and location.** Resolves + validates + pins on EVERY request this handler is asked to open -- including a redirect target, since `HTTPRedirectHandler` builds a brand new `Request` per hop and the `OpenerDirector` re-dispatches it through this same `http_open()`, so this closes the post-redirect TOCTOU too (the previous per-module code only re-validated the resolved final URL's hostname AFTER the redirected connection had already happened). Defined in `zero_sudo/daemon/ssrf_safe_fetch.py:230` as a class; bases: `urllib.request.HTTPHandler`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`, `http_open`.

**See also:** test `test_the_handlers_keep_their_context_label_and_the_https_handler_its_ssl_context (tests/test_ssrf_safe_fetch.py)`.

### `_SSRFSafeHTTPSHandler` [@ANCHOR: zero_sudo:zero_sudo_ssrf_safe_fetch_ssrfsafe_httpshandler]

**Purpose and location.** HTTPS counterpart of `_SSRFSafeHTTPHandler`. `context=self._context` (an `ssl.SSLContext`, defaulted the same way `urllib.request.HTTPSHandler` itself defaults it -- `ssl.create_default_context()`-equivalent, real certificate + hostname verification) is forwarded exactly the way stock `HTTPSHandler.https_open()` forwards it, so nothing about TLS verification changes -- only which raw address the socket dials. Defined in `zero_sudo/daemon/ssrf_safe_fetch.py:252` as a class; bases: `urllib.request.HTTPSHandler`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`, `https_open`.

**See also:** test `test_the_handlers_keep_their_context_label_and_the_https_handler_its_ssl_context (tests/test_ssrf_safe_fetch.py)`.

### `_NoRedirectHandler` [@ANCHOR: zero_sudo:zero_sudo_ssrf_safe_fetch_no_redirect_handler]

**Purpose and location.** Refuses every redirect, so the 3xx answer reaches the caller as an HTTPError (with its Location header). Defined in `zero_sudo/daemon/ssrf_safe_fetch.py:276` as a class; bases: `urllib.request.HTTPRedirectHandler`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `redirect_request`.

**See also:** test `test_a_refused_redirect_reaches_the_caller_as_an_http_error (tests/test_ssrf_safe_fetch.py)`.

### `IrConfigParameterCoreSecrets` [@ANCHOR: zero_sudo:zero_sudo_core_secrets_ir_config_parameter_core_secrets]

**Purpose and location.** Extends the Odoo model "ir.config_parameter" with no new fields and the methods `_get_param`, `_core_secret_db_value`, `create`, `write`, `_check_core_secrets_in_store`, `_register_hook`. Defined in `zero_sudo/models/core_secrets.py:88` as an Odoo model; bases: `models.Model`. (extends `ir.config_parameter`.)

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "ir.config_parameter"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_get_param`, `_core_secret_db_value`, `create`, `write`, `_check_core_secrets_in_store`, `_register_hook`, `_hams_core_secrets_clear_from_database`, `_hams_core_secrets_restore_to_database`.

**See also:** test `test_01_auto_cleanup_tracking (tests/test_facility.py)`.

### `ResUsersCoreSecrets` [@ANCHOR: zero_sudo:zero_sudo_core_secrets_res_users_core_secrets]

**Purpose and location.** Extends the Odoo model "res.users" with no new fields and the methods `_session_token_get_values`. Defined in `zero_sudo/models/core_secrets.py:210` as an Odoo model; bases: `models.Model`. (extends `res.users`.)

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "res.users"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_session_token_get_values`.

**See also:** test `test_flip_keeps_sessions_when_the_store_holds_the_old_value (tests/test_core_secrets_from_store.py)`.

### `ZeroSudoDaemonUtils` [@ANCHOR: zero_sudo:zero_sudo_daemon_utils_zero_sudo_daemon_utils]

**Purpose and location.** Model description: Daemon Management Utilities. Defined in `zero_sudo/models/daemon_utils.py:23` as an Odoo model; bases: `models.AbstractModel`. (model `zero_sudo.daemon.utils`.) It is an abstract model: it has no table and is only mixed into others.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |

**Class attributes.** `_name = "zero_sudo.daemon.utils"`; `_description = "Daemon Management Utilities"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_start_daemon_process`, `_stop_daemon_process`, `_poll_health_check`.

**See also:** test `test_daemon_utils_sys_paths (tests/test_tdd_fixes.py)`.

### `IrHttp` [@ANCHOR: zero_sudo:zero_sudo_ir_http_ir_http]

**Purpose and location.** Extends the Odoo model 'ir.http' with no new fields and the methods `_post_dispatch`, `_is_service_account_cached`, `_is_rpc_path_exempt_from_service_account_block`, `_authenticate`. Defined in `zero_sudo/models/ir_http.py:53` as an Odoo model; bases: `models.AbstractModel`. (extends `ir.http`.) It is an abstract model: it has no table and is only mixed into others.

**Fields.** none declared by this class.

**Class attributes.** `_inherit = 'ir.http'`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_post_dispatch`, `_is_service_account_cached`, `_is_rpc_path_exempt_from_service_account_block`, `_authenticate`.

**See also:** test `test_02_service_account_session_blocked_from_interactive_web_ui (tests/test_controllers.py)`.

### `Module` [@ANCHOR: zero_sudo:zero_sudo_ir_module_module_module]

**Purpose and location.** Extends the Odoo model "ir.module.module" with no new fields and the methods `_register_hook`, `_bootstrap_knowledge_docs`, `_markdown_doc_to_html`, `_is_internal_doc_path`, `_doc_audience_for`, `_is_path_within_module_dir`. Defined in `zero_sudo/models/ir_module_module.py:79` as an Odoo model; bases: `models.Model`. (extends `ir.module.module`.)

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "ir.module.module"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_register_hook`, `_bootstrap_knowledge_docs`, `_markdown_doc_to_html`, `_is_internal_doc_path`, `_doc_audience_for`, `_is_path_within_module_dir`, `_install_single_doc`.

**See also:** test `test_stories_journeys_and_runbooks_directories_are_internal_paths (tests/test_ir_module_doc_helpers.py)`.

### `ZeroSudoKV` [@ANCHOR: zero_sudo:zero_sudo_kv_zero_sudo_kv]

**Purpose and location.** Model description: Zero-Sudo Key-Value Store. Defined in `zero_sudo/models/kv.py:10` as an Odoo model; bases: `models.Model`. (model `zero_sudo.kv`.) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `key` | `Char` | Key (required=`True`; index=`True`) |
| `value` | `Text` | Value |

**Class attributes.** `_name = "zero_sudo.kv"`; `_description = "Zero-Sudo Key-Value Store"`; `_key_uniq = models.Constraint("UNIQUE(key)", "The key must be mathematically unique.")`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_07_common_setup_class_sql (tests/test_facility.py)`.

### `NoisyTable` [@ANCHOR: zero_sudo:zero_sudo_noisy_table_noisy_table]

**Purpose and location.** Model description: Noisy Table. Defined in `zero_sudo/models/noisy_table.py:10` as an Odoo model; bases: `models.Model`. (model `zero_sudo.noisy_table`; default order "name".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Table Name: Name of the PostgreSQL table to ignore in leak detection. (required=`True`; index=`True`) |
| `active` | `Boolean` | If unchecked, it will allow leak detection for this table. (default=`True`; index=`True`) |

**Class attributes.** `_name = "zero_sudo.noisy_table"`; `_description = "Noisy Table"`; `_order = "name"`; `_name_uniq = models.Constraint("UNIQUE(name)", "The table name must be unique!")`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_02_leak_detector_catches_raw_sql (tests/test_facility.py)`.

### `ResUsersZeroSudo` [@ANCHOR: zero_sudo:zero_sudo_res_users_res_users_zero_sudo]

**Purpose and location.** Extends the Odoo model "res.users" with the fields `is_service_account` and the methods `_service_accounts_among_self`, `_notify_security_setting_update`, `create`, `write`. Defined in `zero_sudo/models/res_users.py:13` as an Odoo model; bases: `models.Model`. (extends `res.users`.)

| Field | Type | Meaning |
|---|---|---|
| `is_service_account` | `Boolean` | Is Service Account: Flags this user as an internal service account. Prevents interactive web logins. (default=`False`) |

**Class attributes.** `_inherit = "res.users"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_service_accounts_among_self`, `_notify_security_setting_update`, `create`, `write`.

**See also:** test `test_01_auto_cleanup_tracking (tests/test_facility.py)`.

### `SecurityLog` [@ANCHOR: zero_sudo:zero_sudo_security_log_security_log]

**Purpose and location.** Model description: Zero-Sudo Security Audit Log. Defined in `zero_sudo/models/security_log.py:10` as an Odoo model; bases: `models.Model`. (model `zero_sudo.security.log`; default order "create_date desc".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `user_id` | `Many2one` | Target User (comodel `res.users`; required=`False`; index=`True`) |
| `login` | `Char` | Login Used (index=`True`) |
| `ip_address` | `Char` | IP Address (index=`True`) |
| `user_agent` | `Char` | User Agent |
| `reason` | `Selection` | Reason (values `service_account_blocked`, `privilege_escalation_trip`, `param_access_denied`, `param_write_denied`, `cache_invalidation`; required=`True`; index=`True`) |
| `create_date` | `Datetime` | no label or help in the source (index=`True`) |

**Class attributes.** `_name = "zero_sudo.security.log"`; `_description = "Zero-Sudo Security Audit Log"`; `_order = "create_date desc"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_01_auto_cleanup_tracking (tests/test_facility.py)`.

### `ZeroSudoSecurityUtils` [@ANCHOR: zero_sudo:zero_sudo_security_utils_zero_sudo_security_utils]

**Purpose and location.** Model description: Centralized Security and "
        "Privilege Utilities. Defined in `zero_sudo/models/security_utils.py:54` as an Odoo model; bases: `models.AbstractModel`. (model `zero_sudo.security.utils`.) It is an abstract model: it has no table and is only mixed into others.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |

**Class attributes.** `_name = "zero_sudo.security.utils"`; `_description = "Centralized Security and "
        "Privilege Utilities"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_register_hook`, `_preload_service_uid_cache`, `_get_deterministic_hash`, `_get_service_uid`, `_get_service_env`, `_ground_truth_ids`, `_erase_via_service_account`, `_get_trusted_client_ip`, `_ensure_executable`, `_resolve_dependency_cycle`, `_caller_module_name`, `_is_test_mode`, `_invalidate_model_cache`, `_notify_cache_invalidation`, `_get_param_read_whitelist`, `_get_param_write_whitelist`, `_get_system_param`, `_set_system_param`, `_get_kv`, `_set_kv`, `_get_crypto_secret`.

**See also:** test `test_invalidating_the_utils_cache_does_nothing_when_zero_sudo_is_not_in_the_registry (tests/test_harness_helpers.py)`.

### `_Rejected` [@ANCHOR: zero_sudo:zero_sudo_svg_sanitizer_rejected]

**Purpose and location.** Raised inside one block when it exceeds a cap; the block is dropped. Defined in `zero_sudo/svg_sanitizer.py:355` as a class; bases: `Exception`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_02_budget_element_counts_up_to_the_cap (tests/test_sanitizer_functions.py)`.

### `_CallTotals` [@ANCHOR: zero_sudo:zero_sudo_svg_sanitizer_call_totals]

**Purpose and location.** The class has no docstring; from its definition it provides `__init__`. Defined in `zero_sudo/svg_sanitizer.py:365` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`.

**See also:** test `test_01_call_totals_and_budget_start_at_zero (tests/test_sanitizer_functions.py)`.

### `_Budget` [@ANCHOR: zero_sudo:zero_sudo_svg_sanitizer_budget]

**Purpose and location.** The class has no docstring; from its definition it provides `__init__`, `element`, `spend`. Defined in `zero_sudo/svg_sanitizer.py:372` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`, `element`, `spend`.

**See also:** test `test_02_many_svgs_interleaved_with_elements_are_linear_too (tests/test_svg_review_findings.py)`.
