# 📦 Binary Downloader Module (`binary_downloader`)

*Copyright © Bruce Perens K6BP. AGPL-3.0.*

<system_role>
**Context:** Technical documentation strictly for LLMs and Integrators.
</system_role>

<architecture>
## 1. Architecture
The **Binary Downloader** is a secure, database-backed orchestration module designed to provide static executable dependencies (e.g., `kopia`, `etcd`, `cloudflared`) to other Odoo subsystems. It avoids flat-file manifests to mitigate Arbitrary File Write vulnerabilities.
</architecture>

<security_design>
## 2. Security Design
* **DB-Backed Manifests:** Download targets and cryptographic SHA-256 checksums are stored in the `binary.manifest` model.
* **Least Privilege:** Executes downloads and installations under the dedicated `user_binary_downloader_service` service account.
* **Integrity Enforcement:** Verifies SHA-256 hashes before moving binaries to the execution path (`hams_bin`).
* **Tar Slip Protection:** Implements strict path validation and member name sanitization during archive extraction.
</security_design>

<api>
## 3. API Reference

### `binary.manifest` model
The primary interface for dependency resolution.

#### `ensure_executable(cmd_name)`
`[@ANCHOR: binary_ensure_executable]`
Resolves and ensures a binary is available and executable.

#### `_compute_is_installed()`
`[@ANCHOR: binary_compute_installed]`
Tracks whether a binary is available and executable.

#### `action_install()`
`[@ANCHOR: binary_action_install]`
Triggers installation via the UI.
* **Parameters:** `cmd_name` (string) - The name of the binary (e.g., "kopia").
* **Returns:** `path` (string) - The absolute path to the verified executable.
* **Logic:**
    1. Checks if the binary is already in the system PATH.
    2. If not, searches for a matching `binary.manifest` record.
    3. Downloads, verifies checksum, and extracts/installs if necessary to `/var/lib/odoo/hams_bin/`.

### UI Components
* **List View:** `view_binary_downloader_manifest_list` `[@ANCHOR: test_binary_manifest_views]`
* **Form View:** `view_binary_downloader_manifest_form` `[@ANCHOR: test_binary_manifest_views]`
</api>

<usage>
## 4. Usage Example
```python
bin_path = self.env["binary.manifest"].ensure_executable("kopia")
subprocess.run([bin_path, "--version"], check=True)
```
</usage>

<stories_and_journeys>
## 5. Architectural Stories & Journeys

For detailed narratives and end-to-end workflows, refer to the following:

### Stories
* [Binary Resolution](binary_downloader/docs/stories/binary_resolution.md)
* [UI Installation](binary_downloader/docs/stories/ui_installation.md)
* [Installation Status Check](binary_downloader/docs/stories/is_installed_check.md)

### Journeys
* [Automated Provisioning Flow](binary_downloader/docs/journeys/auto_provisioning_flow.md)
</stories_and_journeys>
