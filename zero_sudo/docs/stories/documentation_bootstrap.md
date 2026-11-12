<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: Centralized Documentation Bootstrap

This story describes how documentation is centrally installed across the platform.

## Automatic Documentation Installation
As a system administrator, I want the module's documentation to be automatically available in the knowledge base upon installation, so that users can access help immediately.

The platform utilizes a documentation bootstrap mechanism provided by the `zero_sudo` module. This ensures that documentation installation is attempted centrally and only after the Odoo registry is fully loaded and all modules are ready.

## Flexible Documentation Support
As a developer, I want my module to support both the community `knowledge` and the enterprise `knowledge` modules for documentation storage, so that the module remains versatile.

The documentation injection logic in `zero_sudo` dynamically checks for the presence of the `knowledge.article` or `knowledge.article` models. If found, it installs the module's guide as specified in the `knowledge_docs` manifest key, ensuring compatibility across different Odoo editions.

## The Process
1. **Module Loading**: Odoo completes its registry load phase.

2. **Hook Execution**: `_register_hook` fires and calls `_bootstrap_knowledge_docs`.

3. **Injection**: the utility safely installs all module docs defined in their `knowledge_docs` manifest arrays.

4. **Per-Doc Install**: each individual entry is installed as its own article.

The entries for steps 2 to 4:

### `ir.module.module._register_hook()` `[@ANCHOR: zero_sudo:ir_module_register_hook]`

`@api.model`, no arguments, returns nothing. Odoo calls it once per registry load, after every module is loaded. It runs core's
`_register_hook` first, then `_bootstrap_knowledge_docs()` unconditionally, so a module installed since the last load gets its
guides on the next load rather than only at install time. It raises nothing of its own; the bootstrap logs and returns on its
failure cases rather than blocking the registry load. Runs under whatever environment the registry load uses and moves to service
accounts itself. See the Injection entry. Tests: `tests/test_tdd_fixes.py`.

### `ir.module.module._bootstrap_knowledge_docs()` `[@ANCHOR: zero_sudo:bootstrap_knowledge_docs]`

`@api.model`, no arguments, returns nothing. Chooses the service account that writes articles
(`knowledge.user_knowledge_service_account` when that record exists, else `zero_sudo.odoo_facility_service_internal`), and returns
quietly, with a warning, when a service uid cannot be resolved (`AccessError`) or when the `knowledge.article` model is not in the
registry. It then reads every installed module (state `installed`, in batches of 1000 ordered by id, as the facility service
account) and collects each manifest's `knowledge_docs` list; a value that is not a list is logged and skipped. For all documents it loads
the stored content hashes and article ids from `zero_sudo.kv` with one query, then calls `_install_single_doc` for each. Side
effects: writes `knowledge.article` rows and `zero_sudo.kv` rows (with `mail_notrack`); nothing is sent. Idempotent: an unchanged
document costs one hash comparison. No explicit locking; concurrent loads on different workers can race, but each write sets
the same final content.

### `ir.module.module._install_single_doc(utils, Article, module_name, doc_info, existing_hashes=None, existing_article_ids=None)` `[@ANCHOR: zero_sudo:install_single_doc]`

| Argument | Type | Meaning |
|---|---|---|
| `utils` | `zero_sudo.security.utils` | the service-account helper, used for the key-value reads and writes |
| `Article` | recordset | `knowledge.article` already bound to the service user |
| `module_name` | `str` | the module whose manifest declared the document |
| `doc_info` | `dict` | the manifest entry: `path` (required), `name`, `icon`, `category`, `public` or `audience`, `help_topic`, `sequence` |
| `existing_hashes` | `dict` or `None` | stored content hashes by key; `None` reads them one by one |
| `existing_article_ids` | `dict` or `None` | stored article ids by key; `None` reads them one by one |

Returns nothing. Skips silently a missing `path`, a `..` segment, and a resolved path outside the module directory
(`_is_path_within_module_dir`); logs an error and skips an unreadable file (`OSError`). Hashes the file bytes plus a version
marker, and does nothing when the hash equals the stored one. Otherwise converts a `.md` file to sanitized HTML, sets
`is_published` and `internal_permission` (`read` for public, `none` for internal) from `classify_doc_audience`, and, only when
the field exists (Odoo Enterprise's Knowledge lacks them), `audience` and `help_topic`. The article is found by the id this
module recorded under its own `(module, name)` key, never by searching the shared `name`, so two modules choosing the same
title cannot overwrite each other. A deleted article is recreated. It then stores the new hash and article id with
`_set_kv`. A manifest `public: True` on a story, journey, runbook or README is logged as an error and installed as internal.

5. **Path Containment** `[@ANCHOR: zero_sudo:is_path_within_module_dir]`: Before a manifest-declared document is read, its resolved path must be the module's own directory or lie beneath it. The check compares against the directory plus a path separator, not a bare string prefix, so a sibling directory whose name merely begins with the module's name (for example `zero_sudo_evil` next to `zero_sudo`) is rejected, as is any absolute path outside the module.
6. **Audience Helpers** `[@ANCHOR: zero_sudo:ir_module_doc_audience_for]` `[@ANCHOR: zero_sudo:ir_module_is_internal_doc_path]` `[@ANCHOR: zero_sudo:ir_module_markdown_doc_to_html]`: three small `ir.module.module` model methods (`@api.model`, no database access, no side effects) that the installer and other code call on the model.
   - `_doc_audience_for(path, doc_info)` returns `"member"` or `"internal"` for a `knowledge_docs` manifest entry. It is the model-level door to `classify_doc_audience`, so callers and tests ask the model instead of repeating the rules. `path` is the entry's path (or empty), `doc_info` its manifest dict (or `None`). Anything not explicitly opted in is internal.
   - `_is_internal_doc_path(path)` returns True when any directory component of `path` (case-insensitive, either slash style, the file name excluded) is `stories`, `journeys` or `runbooks`. Those are never public.
   - `_markdown_doc_to_html(text)` converts markdown (fenced code, tables, table of contents) to HTML and passes it through Odoo's HTML sanitizer, so a `<script>` in the file is removed and literal angle-bracket text in prose becomes escaped text. The installer calls it once per document so every reader gets real HTML.
   - **Tests:** `test_the_model_classifies_a_manifest_entry_like_the_module_function`, `test_stories_journeys_and_runbooks_directories_are_internal_paths`, `test_markdown_becomes_sanitized_html` in `tests/test_ir_module_doc_helpers.py`.
