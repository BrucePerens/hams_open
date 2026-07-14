# Story: UI-Triggered Installation

## Purpose
The `action_install` method allows administrators to manually trigger the download and installation of a binary from the Odoo backend UI.

## Process
1. **User Action:** The administrator clicks the "Install" button on a `binary.manifest` record.
2. **Execution:** The method calls `ensure_executable(self.name)` to perform the resolution and installation logic.
3. **Feedback:** Upon successful completion, it returns a client-side notification to inform the user that the binary was successfully installed.

## Traceability
- **Code:** `action_install` in `models/binary_manifest.py` `[@ANCHOR: binary_action_install]`
- **View:** `view_binary_downloader_manifest_list` and `view_binary_downloader_manifest_form` in `views/binary_manifest_views.xml` `[@ANCHOR: test_binary_manifest_views]`, `[@ANCHOR: binary_downloader:UX_BINARY_INSTALL]`
