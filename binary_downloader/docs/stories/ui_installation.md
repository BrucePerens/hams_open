# Story: UI-Triggered Installation

## Purpose
The `action_install` method allows administrators to manually trigger the download and installation of a binary from the Odoo backend UI.

## Process
1. **User Action:** The administrator clicks the "Install" button on a `binary.manifest` record.
2. **Execution:** The method calls `ensure_executable(self.name)` to perform the resolution and installation logic.
3. **Feedback:** Upon successful completion, it returns a client-side notification to inform the user that the binary was successfully installed.

## Traceability
- **Code:** `action_install` in `models/binary_manifest.py` `COMM_binary_action_install`

- **View:** `view_binary_downloader_manifest_list` and `view_binary_downloader_manifest_form` in `views/binary_manifest_views.xml` `COMM_test_binary_manifest_views`, `binary_downloader:UX_BINARY_INSTALL`

## Anchor reference

One entry per anchor of this story, written from `models/binary_manifest.py`, the form view and the tests.

### `COMM_binary_action_install` [@ANCHOR: COMM_binary_action_install]
Purpose: the "Install Now" button on a manifest. `binary.manifest.action_install()` is a `@public_rpc` method and needs exactly one record (`ensure_one`). Access: raises `UserError` ("You do not have sufficient permissions to install binaries.") unless the caller is in the Binary Downloader Manager group or is an administrator (`env.is_admin()`).

Behaviour: calls `ensure_executable(self.name)`, which resolves the manifest (current company, then global), downloads and verifies the file into `<data_dir>/hams_bin/` if it is not already there. Returns a client action: a "Success" notification ("Installation of X completed successfully.") followed by a page reload. Errors from `ensure_executable` (`UserError`) are passed on. Blocking for the length of the download. Tests: `test_07_action_install` and `test_12_action_install_permissions` in `tests/test_binary_manifest.py`, UI tour `test_binary_install_tour`.

### `COMM_test_binary_manifest_views` [@ANCHOR: COMM_test_binary_manifest_views]
Test anchor for the view-rendering test of the manifest: `test_05_views_render` in `tests/test_binary_manifest.py` loads the list and form views with `get_view` and asserts that the `name` and `url` fields are present. The anchor is also cited in `views/binary_manifest_views.xml` as the test that covers both views (`view_binary_downloader_manifest_list` and `view_binary_downloader_manifest_form`). It checks that the XML compiles and carries the fields, not the permissions of the Install button.

### `binary_downloader:UX_BINARY_INSTALL` [@ANCHOR: binary_downloader:UX_BINARY_INSTALL]
Purpose: marks the user-interface element that starts an install. In `views/binary_manifest_views.xml` the form `view_binary_downloader_manifest_form` has a header with the button "Install Now" (`name="action_install"`, `type="object"`, class `oe_highlight`, accessible label "Install Binary Now"). It is hidden when the record is already installed or not yet saved (`invisible="is_installed or not id"`) and only offered to the groups `binary_downloader.group_binary_downloader_manager` and `base.group_system`.

Special concerns: hiding the button is not the security control; the check lives in `action_install` itself. The element has no code of its own; the behaviour is that of `action_install`. Tests: `test_binary_install_tour` (clicks the button), `test_binary_manifest_views`.
