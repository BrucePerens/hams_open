# Journey: Developer Integrating Documentation

## Summary [@ANCHOR: journey_developer_integration]

This journey follows a module developer who wants their documentation in the shared manual. They write a file in their own module, list it under `knowledge_docs` in the manifest, and the zero_sudo loader installs it as an article when the module is installed and updates it when the file changes. By default the article is administrator-only until the entry marks it public.

This journey follows a developer adding documentation for their module into the global manual.

## Personas
- **Module Developer**: A developer creating a new Odoo module.

## Steps
1. **Documentation Creation**: The developer writes documentation in `data/documentation.html` within their module and adds it to `knowledge_docs` in `__manifest__.py`.
2. **Installation**: When the module is installed (and on every registry load), `ir.module.module._bootstrap_knowledge_docs` scans the installed modules' `knowledge_docs` entries, reads each file inside the module directory, converts Markdown to sanitized HTML, and creates or updates one article per entry when the file's content hash changes. It runs as the Knowledge service account, so no `.sudo()` is used. The article is unpublished and visible only to Manual Administrators until the manifest entry sets `"public": True` and a `"help_topic"`.
   - *Related Story:* `doc_installation.md`
   - *Entry:* `zero_sudo:zero_sudo_doc_installer` below
3. **Verification**: The developer visits `/manual` to ensure their documentation is correctly imported and formatted.
4. **Iterative Update**: If the documentation needs updates, the developer modifies the source file, and the system ensures it's synchronized.

## Entries

### `ir.module.module._bootstrap_knowledge_docs(self)` [@ANCHOR: zero_sudo:zero_sudo_doc_installer]

The loader that installs module documentation into `knowledge.article`. No arguments. It resolves the Knowledge service account (falling back to the zero_sudo facility account), lists the installed modules in batches of 1000, collects each manifest's `knowledge_docs` entries, loads the stored content hashes and article ids in one query, and calls `_install_single_doc` for each entry, which creates or updates the article only when the hash changed. Returns nothing and returns quietly when an account or the Knowledge model is missing. Runs on every registry load. Tests: `tests/test_zero_sudo_doc_installer.py`.
