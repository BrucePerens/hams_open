# Story: Automated Documentation Installation
[@ANCHOR: story_manual_doc_installation]
[@ANCHOR: manual_doc_injection]
[@ANCHOR: manual_doc_auto_install]

This story describes how the system automatically discovers and installs documentation from other modules.

## Scenario
A new Odoo module with a `knowledge_docs` manifest entry is installed.

## Process
1. During server boot or module installation, the `_register_hook` in `ir.module.module` is called.
2. The `_bootstrap_knowledge_docs` method `[@ANCHOR: zero_sudo:zero_sudo_doc_installer]` is triggered.
3. The system identifies available knowledge-base providers (either `knowledge` or Odoo Enterprise `knowledge`).
4. It iterates through installed modules and looks for `knowledge_docs` entries.
5. If found, it reads the content and creates a new article record under a service account context.
6. This ensures documentation is always available regardless of module installation order (soft-dependency pattern).

## Technical Details
- Model: `ir.module.module`
- Methods: `_bootstrap_knowledge_docs`
- Access: Uses `knowledge.user_knowledge_service_account` or `zero_sudo.odoo_facility_service_internal`.
