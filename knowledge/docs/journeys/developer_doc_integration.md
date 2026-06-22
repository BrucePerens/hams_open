# Journey: Developer Integrating Documentation
[@ANCHOR: journey_developer_integration]

This journey follows a developer adding documentation for their module into the global manual.

## Personas
- **Module Developer**: A developer creating a new Odoo module.

## Steps
1. **Documentation Creation**: The developer writes documentation in `data/documentation.html` within their module and adds it to `knowledge_docs` in `__manifest__.py`.
2. **Installation**: When the module is installed, the Knowledge automatically detects the file.
   - *Related Story:* `doc_installation.md`
   - *Anchor:* `[@ANCHOR: zero_sudo:zero_sudo_doc_installer]`
3. **Verification**: The developer visits `/manual` to ensure their documentation is correctly imported and formatted.
4. **Iterative Update**: If the documentation needs updates, the developer modifies the source file, and the system ensures it's synchronized.
