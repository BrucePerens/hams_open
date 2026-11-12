# Story: Automated Documentation Installation

## Summary [@ANCHOR: story_manual_doc_installation]

This story is how documentation reaches the manual without anyone copying it. A module lists files under `knowledge_docs` in its manifest, and `zero_sudo` creates or updates one article per entry when the file's content changes, running as a service account so no `.sudo()` is used. Installed articles are unpublished and administrator-only unless the entry says otherwise, and the order in which modules were installed does not matter.

This story describes how the system automatically discovers and installs documentation from other modules.

## Scenario
A new Odoo module with a `knowledge_docs` manifest entry is installed.

## Process
1. During server boot or module installation, the `_register_hook` in `ir.module.module` is called.
2. The `_bootstrap_knowledge_docs` method `[@ANCHOR: zero_sudo:zero_sudo_doc_installer]` is triggered.
3. The system identifies available knowledge-base providers (either `knowledge` or Odoo Enterprise `knowledge`).
4. It iterates through installed modules and looks for `knowledge_docs` entries.
5. If found, it reads the content and creates a new article record under a service account context. Unless the manifest entry sets `"public": True`, the installed article is unpublished and readable only by Manual Administrators; stories, journeys and runbooks are never published (see "Interoperability" in [the module README](../../README.md)).
6. This ensures documentation is always available regardless of module installation order (soft-dependency pattern).

## Technical Details
- Model: `ir.module.module`
- Methods: `_bootstrap_knowledge_docs`
- Access: Uses `knowledge.user_knowledge_service_account` or `zero_sudo.odoo_facility_service_internal`.

## Anchors

### Injection of manifest documents [@ANCHOR: manual_doc_injection]

Injection is the step that turns a module's `knowledge_docs` manifest entry into an article. For each installed module it reads the file named by the entry's `path` (refusing a path with `..` or one that resolves outside the module directory), converts a `.md` file to sanitized HTML, hashes the content, and creates the article under the service account, or updates the one it made before when the hash changed. The article is looked up by module and entry name, so two modules may use the same title. Tests: `tests/test_features.py` `test_04_doc_installation`.

### Installation at boot and on module install [@ANCHOR: manual_doc_auto_install]

Nothing has to be called by hand. `ir.module.module._register_hook` runs `_bootstrap_knowledge_docs` each time a registry loads, and `post_init_hook` functions such as the one in `hams_rpc_guard` call it right after install, so a module installed before or after `knowledge` gets its article either way. It returns quietly when the Knowledge model or its service account is not there yet. Re-running with unchanged files writes nothing, because each entry's hash is kept in `zero_sudo.kv`. Tests: `tests/test_features.py` `test_04_doc_installation`.
