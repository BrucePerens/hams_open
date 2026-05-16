# Documentation Injection Stories

## Automatic Documentation Installation
As a system administrator, I want the module's documentation to be automatically available in the knowledge base upon installation, so that users can access help immediately.

The module utilizes the documentation bootstrap mechanism ([@ANCHOR: documentation_bootstrap]) provided by the `zero_sudo` module. This ensures that documentation installation is attempted centrally and only after the Odoo registry is fully loaded and all modules are ready.

## Flexible Documentation Support
As a developer, I want my module to support both the community `manual_library` and the enterprise `knowledge` modules for documentation storage, so that the module remains versatile.

The documentation injection logic ([@ANCHOR: documentation_injection]) in `zero_sudo` dynamically checks for the presence of the `knowledge.article` or `manual.article` models. If found, it installs the module's guide as specified in the `knowledge_docs` manifest key, ensuring compatibility across different Odoo editions.
