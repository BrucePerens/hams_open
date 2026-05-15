# Story: Centralized Documentation Bootstrap `[@ANCHOR: story_zero_sudo_doc_installer]`

This story describes how documentation is centrally installed across the platform.

## The Process
1. **Module Loading**: Odoo completes its registry load phase.
2. **Hook Execution**: `_register_hook` fires and calls `_bootstrap_knowledge_docs` `[@ANCHOR: zero_sudo_doc_installer]`.
3. **Injection**: The utility safely installs all module docs defined in their `knowledge_docs` manifest arrays.

## Verification
- **Automated Test**: `test_09_bootstrap_knowledge_docs` in `test_security_utils.py` `[@ANCHOR: test_zero_sudo_doc_installer]` verifies that documentation is correctly discovered and installed.
