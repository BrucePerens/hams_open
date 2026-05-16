# Documentation Setup Journey

This journey describes how the `test_real_transaction` module ensures its documentation is properly installed.

1.  **Server Start/Module Loading**: The Odoo server starts or the module is installed/updated.
2.  **Registry Loading**: Odoo builds the model registry.
3.  **Bootstrap Trigger**: Once the registry is ready, the `ir.module.module` model from `zero_sudo` executes its `_register_hook` ([@ANCHOR: documentation_bootstrap]).
4.  **Manifest Processing**: The hook scans all installed modules for the `knowledge_docs` key in their manifest files ([@ANCHOR: documentation_injection]).
5.  **Service Account Context**: If the API is available, the system switches to a secure service account context (usually `manual_library.user_manual_library_service_account`) to perform the installation.
6.  **Idempotent Check**: The system checks if the "Real Transaction Testing Facility Guide" already exists to avoid duplicate entries.
7.  **Content Loading**: The documentation content is read from `hams_test/data/documentation.html`.
8.  **Record Creation**: A new article is created and published in the knowledge base, making it accessible to authorized users.
