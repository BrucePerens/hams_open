# Binary Downloader Module (`binary_downloader`)

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).*

The **Binary Downloader** is a secure, database-backed orchestration module designed to provide static executable dependencies (e.g., `kopia`, `etcd`, `cloudflared`) to other Odoo subsystems. It avoids flat-file manifests to mitigate Arbitrary File Write vulnerabilities.

---

# Technical Documentation

<system_role>
**Context:** Technical documentation strictly for LLMs and Integrators.
</system_role>

<security_design>
## 1. Security Design
* **DB-Backed Manifests:** Download targets and cryptographic SHA-256 checksums are stored in the `binary.manifest` model.
* **Least Privilege:** Executes downloads and installations under the dedicated `user_binary_downloader_service` service account.
* **Integrity Enforcement:** Verifies SHA-256 hashes before moving binaries to the execution path (`hams_bin`).
* **Tar Slip Protection:** Implements strict path validation and member name sanitization during archive extraction.
* **Timeouts:** All network operations have strict timeouts (15s for HEAD, 600s for GET) to prevent resource exhaustion.
* **Permissions:** Target directory and binaries are set to `0o750` to restrict access.
</security_design>

<api>
## 2. API Reference

### `binary.manifest` model
The primary interface for dependency resolution.

#### `ensure_executable(cmd_name)`
`[@ANCHOR: binary_ensure_executable]`

Resolves and ensures a binary is available and executable. Returns the absolute path to the binary.

#### `_compute_is_installed()`
`[@ANCHOR: binary_compute_installed]`

Tracks whether a binary is available in the system PATH or `hams_bin` and is executable.

#### `action_install()`
`[@ANCHOR: binary_action_install]`

Triggers installation via the UI. Restricted to members of `Binary Downloader Manager` or Administrators.

* **Logic:**
    1. Checks if the binary is already available in the system PATH or the local `hams_bin` cache.
    2. If not, it executes an ethical HEAD request to check for server availability and ETags.
    3. Downloads the binary/archive to a temporary file within `hams_bin`.
    4. Verifies the SHA-256 checksum of the downloaded file.
    5. If it's a `.tar.gz`, it extracts the specified `extract_member` (or the binary name) into `hams_bin`.
    6. Extraction includes strict Tar Slip protection, symlink/hardlink rejection, and file-type validation.
    7. Sets file permissions to `0o750` for security.

### UI Components
* **List View:** `view_binary_downloader_manifest_list` `[@ANCHOR: test_binary_manifest_views]`
* **Form View:** `view_binary_downloader_manifest_form` `[@ANCHOR: test_binary_manifest_views]`
* **Menu:** Located under **Settings -> Administration -> Binary Manifests**.
</api>

<usage>
## 3. Usage Example
```python
bin_path = self.env["binary.manifest"].ensure_executable("kopia")
# Verified by [@ANCHOR: test_binary_manifest_standard]
subprocess.run([bin_path, "--version"], check=True)
```
</usage>

<stories_and_journeys>
## 4. Architectural Stories & Journeys

For detailed narratives and end-to-end workflows, refer to the following:

### Stories
* [Binary Resolution](docs/stories/binary_resolution.md)
* [UI Installation](docs/stories/ui_installation.md)
* [Installation Status Check](docs/stories/is_installed_check.md)

### Journeys
* [Automated Provisioning Flow](docs/journeys/auto_provisioning_flow.md)
</stories_and_journeys>

<semantic_anchors>
## 5. Semantic Anchors
- `[@ANCHOR: binary_ensure_executable]` - Core binary resolution method.
- `[@ANCHOR: binary_compute_installed]` - Installation status computation.
- `[@ANCHOR: binary_action_install]` - UI installation trigger.
- `[@ANCHOR: UX_BINARY_INSTALL]` - UI elements for installation.
- `[@ANCHOR: test_binary_manifest_standard]` - Standard unit tests.
- `[@ANCHOR: test_binary_install_tour]` - UI tour for binary installation.
- `[@ANCHOR: test_binary_manifest_views]` - View rendering tests.
</semantic_anchors>
