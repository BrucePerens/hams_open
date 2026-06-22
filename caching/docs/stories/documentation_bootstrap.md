# Story: Documentation Bootstrap

## Context
In complex Odoo environments, documentation needs to be easily accessible to administrators and integrators.

## The Problem
Manually installing documentation into Odoo's Knowledge or Knowledge apps is error-prone and often forgotten.

## The Solution
The `caching` module utilizes the automated "soft-dependency" documentation bootstrap provided by the `zero_sudo` module ([@ANCHOR: caching_docs_bootstrap]).

1. **Manifest Declaration**: The documentation is declared in the `knowledge_docs` array within `caching/__manifest__.py`.
2. **Detection**: During module installation or registry loading, the `zero_sudo` module's `_bootstrap_knowledge_docs` method checks for compatible documentation models (like `knowledge.article` or `knowledge.article`).
3. **Security**: It uses the system's internal mechanisms to perform the installation, ensuring it has the necessary permissions even in restricted environments without requiring `sudo()`.
4. **Installation**: It reads the `caching/data/documentation.html` file and creates a new Article if one doesn't already exist.
5. **Idempotency**: The system ensures that it doesn't create duplicate articles on every server restart.

## Impact
Technical documentation is always available directly within the Odoo interface for systems that support it, without requiring the `caching` module to have a hard dependency on documentation modules.
