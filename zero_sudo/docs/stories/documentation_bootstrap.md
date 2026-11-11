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

2. **Hook Execution** `[@ANCHOR: zero_sudo:ir_module_register_hook]`: `_register_hook` fires and calls `_bootstrap_knowledge_docs`.

3. **Injection** `[@ANCHOR: zero_sudo:bootstrap_knowledge_docs]`: The utility safely installs all module docs defined in their `knowledge_docs` manifest arrays.

4. **Per-Doc Install** `[@ANCHOR: zero_sudo:install_single_doc]`: Each individual entry in a module's `knowledge_docs` array is installed (or updated, if its content hash changed) as its own `knowledge.article`, defaulting to unpublished (internal-only) unless the manifest entry explicitly opts into `"public": True`.

5. **Path Containment** `[@ANCHOR: zero_sudo:is_path_within_module_dir]`: Before a manifest-declared document is read, its resolved path must be the module's own directory or lie beneath it. The check compares against the directory plus a path separator, not a bare string prefix, so a sibling directory whose name merely begins with the module's name (for example `zero_sudo_evil` next to `zero_sudo`) is rejected, as is any absolute path outside the module.
6. **Audience Helpers** `[@ANCHOR: zero_sudo:ir_module_doc_audience_for]` `[@ANCHOR: zero_sudo:ir_module_is_internal_doc_path]` `[@ANCHOR: zero_sudo:ir_module_markdown_doc_to_html]`: three small `ir.module.module` model methods (`@api.model`, no database access, no side effects) that the installer and other code call on the model.
   - `_doc_audience_for(path, doc_info)` returns `"member"` or `"internal"` for a `knowledge_docs` manifest entry. It is the model-level door to `classify_doc_audience`, so callers and tests ask the model instead of repeating the rules. `path` is the entry's path (or empty), `doc_info` its manifest dict (or `None`). Anything not explicitly opted in is internal.
   - `_is_internal_doc_path(path)` returns True when any directory component of `path` (case-insensitive, either slash style, the file name excluded) is `stories`, `journeys` or `runbooks`. Those are never public.
   - `_markdown_doc_to_html(text)` converts markdown (fenced code, tables, table of contents) to HTML and passes it through Odoo's HTML sanitizer, so a `<script>` in the file is removed and literal angle-bracket text in prose becomes escaped text. The installer calls it once per document so every reader gets real HTML.
   - **Tests:** `test_the_model_classifies_a_manifest_entry_like_the_module_function`, `test_stories_journeys_and_runbooks_directories_are_internal_paths`, `test_markdown_becomes_sanitized_html` in `tests/test_ir_module_doc_helpers.py`.
