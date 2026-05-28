# Story: Centralized Documentation Bootstrap `[@ANCHOR: story_zero_sudo_doc_installer]`

This story describes how documentation is centrally installed across the platform.

## Automatic Documentation Installation
As a system administrator, I want the module's documentation to be automatically available in the knowledge base upon installation, so that users can access help immediately.

The platform utilizes a documentation bootstrap mechanism (`[@ANCHOR: documentation_bootstrap]`) provided by the `zero_sudo` module. This ensures that documentation installation is attempted centrally and only after the Odoo registry is fully loaded and all modules are ready.

## Flexible Documentation Support
As a developer, I want my module to support both the community `manual_library` and the enterprise `knowledge` modules for documentation storage, so that the module remains versatile.

The documentation injection logic (`[@ANCHOR: documentation_injection]`) in `zero_sudo` dynamically checks for the presence of the `knowledge.article` or `manual.article` models. If found, it installs the module's guide as specified in the `knowledge_docs` manifest key, ensuring compatibility across different Odoo editions.

## The Process
1. **Module Loading**: Odoo completes its registry load phase.
2. **Hook Execution**: `_register_hook` fires and calls `_bootstrap_knowledge_docs` `[@ANCHOR: zero_sudo:zero_sudo_doc_installer]`.
3. **Injection**: The utility safely installs all module docs defined in their `knowledge_docs` manifest arrays.

## Verification
- **Automated Test**: `test_09_bootstrap_knowledge_docs` in `test_security_utils.py` (`[@ANCHOR: zero_sudo:test_zero_sudo_doc_installer]`) verifies that documentation is correctly discovered and installed.
