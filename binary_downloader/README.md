# Binary Downloader Module (`binary_downloader`)

*Copyright © Bruce Perens K6BP.*
SPDX-License-Identifier: AGPL-3.0-or-later

The **Binary Downloader** is a secure, database-backed orchestration module designed to provide static executable dependencies (e.g., `kopia`, `etcd`, `cloudflared`) to other Odoo subsystems. It implements a robust lifecycle management system for external tools while maintaining strict security standards.

This module is fully compliant with the **Zero-Sudo** mandate and uses micro-privilege service accounts for all operations. Zero-Sudo is this platform's rule that code never calls Odoo's `.sudo()` (the platform linter rejects it); privileged work instead runs as a dedicated service-account user through `with_user()`. Here that account is `user_binary_downloader_service`, called micro-privilege because its only assigned group is Binary Downloader Manager. It supports raw binaries, Gzipped Tarballs, and ZIP archives.

The module supports raw binaries as well as compressed archives (`.tar.gz` and `.zip`). When using archives, you must specify the member to be extracted. The installation process is protected by database-level advisory locks to prevent concurrent write operations.

## User Guide

### How it works
This module acts as a "package manager" for Odoo. When another part of the system needs a program (like a backup tool), it checks whether a verified copy is already in Odoo's private binary directory (`hams_bin`). If not, it safely downloads it from a pre-defined trusted source, checks its digital fingerprint (checksum) to ensure it hasn't been tampered with, and makes it available for use.

### Managing Binaries
1. Open the **Binary Manifests** menu. It is declared as a top-level menu (no parent), not under **Settings -> Technical**.
2. You will see a list of required tools. The **Installed** column shows if they are ready: it is checked when a program of that name is on the system `PATH`, or when the manifest's file in `hams_bin` exists and is executable.
3. If a tool is missing, click the **Install** button. Only members of the Binary Downloader Manager group or administrators can install; anyone else gets a permission error. Because a program found on the `PATH` counts as installed, the button is hidden for it, even though `ensure_executable` (below) never uses the `PATH` copy.

### Tenant-Specific Binaries
In a multi-tenant environment, you can assign specific versions of software to different websites:
1. Go to **Settings -> Technical -> Station Executables** (the list it opens is titled "Tenant Links"). In this module a tenant, a station and a website are the same thing: the **Tenant / Website** a link is assigned to.
2. Create a new link between a **Tenant / Website** and a **Software** manifest. Each website can have only one link per manifest.
3. Select the **Active Version** you wish to use (a `binary.version` record of that manifest, defined under **Settings -> Technical -> Version Pool**).
4. The system will automatically create a secure "shortcut" (symlink) for that website to use, at `<data_dir>/tenant_bins/site_<website id>/<binary name>`, pointing at the version's file in the central pool. If that version has not been downloaded yet, it is downloaded and verified first. The symlink is re-pointed whenever **Active Version** changes and removed when the link is deleted.
5. Use the **1-Click Upgrade to Latest** button to quickly move a tenant to the newest available version: the version of that manifest with the latest **Upstream Release Date** (ties go to the most recently created record). If the tenant already uses it, nothing changes and an "Up to Date" notice is shown.

---

# Technical Documentation

<system_role>
**Context:** Technical documentation strictly for developers, LLMs, and System Integrators.
</system_role>

<security_design>
## 1. Security Design
* **DB-Backed Manifests:** Download targets and cryptographic SHA-256 checksums are stored in the `binary.manifest` model, preventing reliance on insecure flat-file manifests. The shipped global manifests (`kopia`, `etcd`, `cloudflared`) are loaded from `data/binary_manifest_data.xml` outside a `noupdate` block, so a module upgrade resets those three records to the file's values and undoes any UI edit to them. A company-specific manifest of the same name is not reset and takes precedence for that company.
* **Least Privilege:** Executes downloads and installations under the dedicated `user_binary_downloader_service` service account. The module is fully compliant with the **Zero-Sudo** mandate.
* **Integrity Enforcement:** Verifies SHA-256 hashes before moving binaries to the execution path (`hams_bin`). For an archive, the hash checked is that of the whole downloaded archive. On later calls, an existing raw binary is re-hashed and re-downloaded if it no longer matches; an existing file extracted from an archive is trusted as is (it is written atomically, through a temporary file and a rename, so a partial write never sits at the final path).
* **Concurrency Protection:** Implements PostgreSQL **advisory locks** (via `pg_advisory_xact_lock`) during the installation process to prevent race conditions and file corruption when multiple Odoo workers trigger installations simultaneously.
* **Archive Security (Tar/Zip Slip):** "Tar Slip"/"Zip Slip" is an attack in which an archive member's path (for example `../../x`) makes extraction write outside the intended directory. Only one member is ever extracted: the first whose name equals `extract_member` or ends in `/<extract_member>`. Its bytes are written to the computed target filename in `hams_bin`, so the member's own path never decides where the file lands, regardless of any directory structure within the archive (`os.path.basename` is used only to skip directory entries). Symbolic links and hard links are strictly forbidden: if the selected member is a link, installation is refused.
* **Timeouts:** All network operations have strict timeouts (15s for both the HEAD and the GET request) to prevent resource exhaustion and hanging threads. A failed HEAD request is only logged; the GET request decides success.
* **Download Restrictions:** Only `https://` URLs are accepted, both when a manifest or version is saved and again at download time. Before connecting, the host name must resolve only to public addresses: loopback, link-local, private-use, multicast and reserved addresses are rejected, and the same check is applied to wherever a redirect lands (a redirect to a non-`https://` URL is also rejected). This blocks Server-Side Request Forgery (SSRF), where a manifest URL would make the Odoo server fetch from internal services. A download larger than 1 GiB is aborted before its checksum is checked.
* **Permissions:** Target directory (`hams_bin`) and binaries are set to `0o750` to restrict execution and access.
* **Multi-Company & Multi-Website Isolation:** Binaries are stored in a shared system directory (`hams_bin`), but their filenames are version-aware: `<name>_<id>`, where `<id>` is the first 16 hex digits of the SHA-256 of `<name>_<checksum>`, so a new checksum gives a new file. The `binary.manifest` model supports `company_id`, allowing companies to define their own specific binary requirements or versions (a binary name is unique per company). The `ensure_executable` method prioritizes the current company's manifest and falls back only to a global manifest (one with no company); another company's manifest is never used.
</security_design>

<api>
## 2. API Reference

### `binary.manifest` model
**Import Path:** `odoo.addons.binary_downloader.models.binary_manifest`
The primary interface for dependency resolution.

#### `ensure_executable(cmd_name)`
`[@ANCHOR: COMM_binary_ensure_executable]`

Resolves and ensures a binary is available and executable. Returns the absolute path to the binary. It looks up the manifest for `cmd_name` (current company first, then global) as the module's service account, then returns that manifest's file in Odoo's private binary directory (`hams_bin`) if it is already there and valid. If not found, it attempts an automatic download and installation. It deliberately does not consult the system `PATH`, so a same-named program elsewhere on the server cannot shadow the verified copy. Callers that prefer a `PATH` copy use `zero_sudo.security.utils._ensure_executable`, which tries `shutil.which()` first and calls this method only when a service account is given.

**Parameters:**
- `cmd_name` (str): The name of the command to ensure (e.g., `"kopia"`).

**Returns:**
- Absolute path (str) to the verified executable.

**Raises:**
- `ValidationError`: If the command name is invalid (empty, contains slashes or backslashes, or is `.` or `..`). Manifest constraints (e.g., missing `extract_member` for tarballs) raise `ValidationError` when the manifest is saved, not here.
- `UserError`: If the manifest is missing, the platform is unsupported (anything but Linux on x86_64, aarch64 or armv7l), checksum/integrity checks fail, the download is refused by the restrictions above, the archive member is not found, or the download or extraction fails.

#### `_compute_is_installed()`
`[@ANCHOR: COMM_binary_compute_installed]`

Tracks whether a binary is available in the system `PATH` or `hams_bin` and has appropriate execution permissions.

#### `action_install()`
`[@ANCHOR: COMM_binary_action_install]`

Triggers manual installation via the UI. Raises `UserError` unless the user is in the Binary Downloader Manager group or is an administrator.

* **Logic:**
    1. Checks if the binary is already available and valid.
    2. If not, downloads, verifies checksum, and extracts/installs if necessary to `<data_dir>/hams_bin/` (`data_dir` from the Odoo configuration, default `/var/lib/odoo`). It does this by calling `ensure_executable`.

#### `unlink()`
`[@ANCHOR: COMM_binary_unlink]`

Overrides the standard unlink method to safely remove binary files when a manifest is deleted. A file is removed only when no `binary.manifest` or `binary.version` record outside the ones being deleted still maps to it (same name and checksum).

### `binary.tenant.link` model
**Import Path:** `odoo.addons.binary_downloader.models.binary_tenant_link`

Manages the assignment of specific binary versions to different tenants (websites).

#### `apply_symlink()`
`[@ANCHOR: COMM_tenant_link_apply]`

Creates a secure symbolic link in a website-specific directory (`<data_dir>/tenant_bins/site_<website id>/`) pointing to the assigned binary version, first downloading that version to the central pool if needed. Called automatically on create and whenever `active_version_id` changes.

#### `action_upgrade_to_latest()`
`[@ANCHOR: COMM_tenant_link_upgrade]`

Automatically upgrades the tenant's assigned binary to the newest available version in the central pool, where newest means the latest `release_date`, ties broken by highest `id`. Returns `False` if the manifest has no versions.

### `binary.version` model
**Import Path:** `odoo.addons.binary_downloader.models.binary_version`

Maintains a central pool of binary versions for assignment to tenants. The "central pool" is not a separate store: each version's file is kept in the same `hams_bin` directory as manifest installs, under the same `<name>_<id>` naming, so a version and a manifest with the same name and checksum share one file.

#### `_get_central_path()`
`[@ANCHOR: COMM_binary_version_path]`

Returns the absolute path to the stored binary version in the central pool.

#### `action_download_to_pool()`
`[@ANCHOR: COMM_binary_version_download]`

Retrieves the selected binary version from the remote source and stores it in the central pool. Raises `UserError` unless the user is in the Binary Downloader Manager group or is an administrator.

#### `action_notify_tenants()`
`[@ANCHOR: COMM_binary_version_notify]`

Triggers PagerDuty alerts to notify stations and tenants when a new binary version becomes available. "PagerDuty" here is this platform's own `pager_duty` Odoo module: the method creates one `pager.incident` record (source `binary_update`, severity `medium`) for each tenant link of the manifest that is not already on this version. Raises `UserError` unless the user is in the Binary Downloader Manager group or is an administrator.

### `binary_downloader.mixin`
**Import Path:** `odoo.addons.binary_downloader.models.binary_utils`

A utility mixin providing common file operations.

#### `_download_and_extract()`
`[@ANCHOR: COMM_binary_utils_download]`

Safely downloads and extracts binaries from raw URLs, tarballs, or ZIP archives.

#### `_get_target_filename()`
`[@ANCHOR: COMM_binary_utils_filename]`

Generates a secure, version-aware filename for the downloaded binary.

#### `_unlink_binary_file()`
`[@ANCHOR: COMM_binary_utils_unlink]`

Safely removes a binary file from the filesystem.
</api>

<usage>
## 3. Usage Example
```python
# To be called by other modules needing a binary dependency
bin_path = self.env["binary.manifest"].ensure_executable("kopia")
# Verified by [@ANCHOR: COMM_test_binary_manifest_standard]

# Verified by [@ANCHOR: COMM_test_binary_manifest_integration]
subprocess.run([bin_path, "--version"], check=True, shell=False)
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
- `[@ANCHOR: COMM_binary_ensure_executable]` - Core binary resolution method.

- `[@ANCHOR: COMM_binary_compute_installed]` - Installation status computation.

- `[@ANCHOR: COMM_binary_action_install]` - UI installation trigger.

- `[@ANCHOR: binary_downloader:UX_BINARY_INSTALL]` - UI elements for installation.

- `[@ANCHOR: COMM_test_binary_manifest_standard]` - Standard unit tests.

- `[@ANCHOR: COMM_test_binary_manifest_integration]` - Unmocked physical integration tests.

- `[@ANCHOR: COMM_pure_python_symlink_engine]` - Pure Python symlink engine.

- `[@ANCHOR: COMM_binary_resolution]` - Binary resolution tenant isolation.

- `[@ANCHOR: test_binary_install_tour]` - UI tour for binary installation.

- `[@ANCHOR: test_binary_manifest_views]` - View rendering tests.

- `[@ANCHOR: binary_version_download_pool]` - Versioned binary download.

- `[@ANCHOR: test_binary_version_standard]` - Standard unit tests for versioning.

- `[@ANCHOR: test_tenant_link_form]` - View rendering tests for tenant links.

- `[@ANCHOR: test_binary_version_form]` - View rendering tests for binary versions.

## 6. External Dependencies
* `python`: `[]`
</semantic_anchors>
