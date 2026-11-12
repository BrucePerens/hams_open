<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: Multi-Website Awareness `[@ANCHOR: zero_sudo:COMM_story_multi_website]`

This story describes how the `zero_sudo` security core behaves when one Odoo instance serves several websites or companies. The
short rule: everything `zero_sudo` guards is a property of the whole instance, so no check, parameter or table below is filtered
by `website_id` or `company_id`, and a developer must not add such a filter. Tests: `zero_sudo/tests/test_security_utils.py` and
`zero_sudo/tests/test_controllers.py`.

## Web Login Isolation

### The service-account check in `web_login` `[@ANCHOR: zero_sudo:COMM_web_login_interceptor_check]`

Where: `ZeroSudoHome.web_login(redirect=None, login=None, **kw)` in `controllers/main.py`, after core's `web_login` has run.

| Input | Meaning |
|---|---|
| `request.session.uid` | the user the login just authenticated; no uid means the check does nothing |
| `login` (or `request.params["login"]`) | the typed login name, recorded only in the audit row |

Reads `res_users.is_service_account` for that uid with one parameterised SQL statement, on purpose and not through `.sudo()` or
the ORM (the audited pattern of ADR 0005, so no ACL is bypassed with a superuser). When the flag is set, a
`zero_sudo.security.log` row is written through the facility service account with reason `service_account_blocked`, the user id,
the typed login, the trusted client IP (`_get_trusted_client_ip`, not the raw peer, which is loopback behind the tunnel) and the
user agent; then the session is logged out and the response is a redirect to `/web/login?error=access_denied_service`. Any other
user gets core's response unchanged. It runs on the session, so the website the visitor reached is irrelevant: a service
account is refused on every website of the instance. No password or token is logged. Raises nothing of its own.

## Documentation Bootstrap

### The documentation installer `[@ANCHOR: zero_sudo:COMM_zero_sudo_doc_installer]`

The installer is `ir.module.module._bootstrap_knowledge_docs` (see [documentation_bootstrap.md](documentation_bootstrap.md); the
anchor itself is placed in `zero_sudo/__init__.py`). It takes no website argument and writes each manifest `knowledge_docs`
entry as one `knowledge.article` in the instance's central knowledge base, so a guide is available across all websites, and
whether a visitor sees it depends on the article's published flag and permission, not on a website. It sets no website field of
its own; if the article model offers website-level isolation, the platform's default visibility applies and `zero_sudo` does not
override it. Side effects and failure cases are listed in the documentation bootstrap story. Idempotent: an unchanged document
is skipped by content hash.

## System Parameters

### `_get_system_param(key, default=None)` `[@ANCHOR: zero_sudo:COMM_get_system_param]`

| Argument | Type | Meaning |
|---|---|---|
| `key` | `str` | the `ir.config_parameter` key |
| `default` | any | returned when the parameter is not set |

Returns the parameter's value (or `default`) read through `zero_sudo.config_service_internal`. A key is served only when it is in
`_get_param_read_whitelist()`. Any other key writes a `zero_sudo.security.log` row (`param_access_denied`, with the key in the
login column) and raises `AccessError`; when the name also contains `secret`, `key`, `password`, `token`, `auth`, `crypt` or
`cert` the message says it matches restricted cryptographic patterns (the mechanical secret block). Parameters are
`ir.config_parameter` rows and so global to the database: the whitelist is what keeps a multi-website instance from exposing an
unintended one to a website's code. The result is cached by the distributed cache decorator. Writing is
`_set_system_param`, with its own, shorter write whitelist.

## Global Models

Certain models in `zero_sudo` are logically global and do not track `website_id` or `company_id`. Website isolation is
maintained by what the rows hold, not by filtering them.

### KV store `zero_sudo.kv` `[@ANCHOR: zero_sudo:COMM_zero_sudo_kv_global]`

Fields: `name` (char, defaults to the model description), `key` (char, required, indexed, unique) and `value` (text). A SQL
`UNIQUE(key)` constraint rejects a duplicate key with "The key must be mathematically unique." The model stores platform-wide
technical state and cryptographic flags for service accounts that act across all companies and websites, for example the
content hash and article id of each installed guide. It is written through `_set_kv` (one Postgres procedure call) and read
through `_get_kv`, both on the facility service account; code does not touch the model directly. Because a key is unique
across the instance, a feature that needs per-website state must put the website in the key name.

### Noisy tables `zero_sudo.noisy_table` `[@ANCHOR: zero_sudo:COMM_zero_sudo_noisy_table_global]`

Fields: `name` (char, required, indexed, unique: the PostgreSQL table name) and `active` (boolean, default true, indexed;
unchecking it makes leak detection watch that table again). Ordered by name. The rows list physical PostgreSQL tables that the
leak-detection engine ignores because they change constantly without leaking anything. Physical tables belong to the whole
database cluster, so the list is global by nature and cannot be split by website or company. A duplicate name is rejected
("The table name must be unique!"). Seed rows come from `data/noisy_table_data.xml`.
