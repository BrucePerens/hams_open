# Documentation Injection Stories

## Automatic Documentation Installation
As a system administrator, I want the module's documentation to be automatically available in the knowledge base upon installation, so that users can access help immediately.

The module implements a documentation bootstrap mechanism ([@ANCHOR: documentation_bootstrap]) using the `_register_hook` method. This ensures that documentation installation is attempted only after the Odoo registry is fully loaded and all modules are ready.

## Flexible Documentation Support
As a developer, I want my module to support both the community `manual_library` and the enterprise `knowledge` modules for documentation storage, so that the module remains versatile.

The documentation injection logic ([@ANCHOR: documentation_injection]) dynamically checks for the presence of the `knowledge.article` API. If found, it installs the module's guide using a secure service account context ([@ANCHOR: user_real_transaction_service]), ensuring compatibility across different Odoo editions.
