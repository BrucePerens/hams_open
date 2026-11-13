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
`COMM_binary_ensure_executable`

Resolves and ensures a binary is available and executable. Returns the absolute path to the binary. It looks up the manifest for `cmd_name` (current company first, then global) as the module's service account, then returns that manifest's file in Odoo's private binary directory (`hams_bin`) if it is already there and valid. If not found, it attempts an automatic download and installation. It deliberately does not consult the system `PATH`, so a same-named program elsewhere on the server cannot shadow the verified copy. Callers that prefer a `PATH` copy use `zero_sudo.security.utils._ensure_executable`, which tries `shutil.which()` first and calls this method only when a service account is given.

**Parameters:**
- `cmd_name` (str): The name of the command to ensure (e.g., `"kopia"`).

**Returns:**
- Absolute path (str) to the verified executable.

**Raises:**
- `ValidationError`: If the command name is invalid (empty, contains slashes or backslashes, or is `.` or `..`). Manifest constraints (e.g., missing `extract_member` for tarballs) raise `ValidationError` when the manifest is saved, not here.
- `UserError`: If the manifest is missing, the platform is unsupported (anything but Linux on x86_64, aarch64 or armv7l), checksum/integrity checks fail, the download is refused by the restrictions above, the archive member is not found, or the download or extraction fails.

#### `_compute_is_installed()`
`COMM_binary_compute_installed`

Tracks whether a binary is available in the system `PATH` or `hams_bin` and has appropriate execution permissions.

#### `action_install()`
`COMM_binary_action_install`

Triggers manual installation via the UI. Raises `UserError` unless the user is in the Binary Downloader Manager group or is an administrator.

* **Logic:**
    1. Checks if the binary is already available and valid.
    2. If not, downloads, verifies checksum, and extracts/installs if necessary to `<data_dir>/hams_bin/` (`data_dir` from the Odoo configuration, default `/var/lib/odoo`). It does this by calling `ensure_executable`.

#### `unlink()`
`COMM_binary_unlink`

Overrides the standard unlink method to safely remove binary files when a manifest is deleted. A file is removed only when no `binary.manifest` or `binary.version` record outside the ones being deleted still maps to it (same name and checksum).

### `binary.tenant.link` model
**Import Path:** `odoo.addons.binary_downloader.models.binary_tenant_link`

Manages the assignment of specific binary versions to different tenants (websites).

#### `apply_symlink()`
`COMM_tenant_link_apply`

Creates a secure symbolic link in a website-specific directory (`<data_dir>/tenant_bins/site_<website id>/`) pointing to the assigned binary version, first downloading that version to the central pool if needed. Called automatically on create and whenever `active_version_id` changes.

#### `action_upgrade_to_latest()`
`COMM_tenant_link_upgrade`

Automatically upgrades the tenant's assigned binary to the newest available version in the central pool, where newest means the latest `release_date`, ties broken by highest `id`. Returns `False` if the manifest has no versions.

### `binary.version` model
**Import Path:** `odoo.addons.binary_downloader.models.binary_version`

Maintains a central pool of binary versions for assignment to tenants. The "central pool" is not a separate store: each version's file is kept in the same `hams_bin` directory as manifest installs, under the same `<name>_<id>` naming, so a version and a manifest with the same name and checksum share one file.

#### `_get_central_path()`
`COMM_binary_version_path`

Returns the absolute path to the stored binary version in the central pool.

#### `action_download_to_pool()`
`COMM_binary_version_download`

Retrieves the selected binary version from the remote source and stores it in the central pool. Raises `UserError` unless the user is in the Binary Downloader Manager group or is an administrator.

#### `action_notify_tenants()`
`COMM_binary_version_notify`

Triggers PagerDuty alerts to notify stations and tenants when a new binary version becomes available. "PagerDuty" here is this platform's own `pager_duty` Odoo module: the method creates one `pager.incident` record (source `binary_update`, severity `medium`) for each tenant link of the manifest that is not already on this version. Raises `UserError` unless the user is in the Binary Downloader Manager group or is an administrator.

### `binary_downloader.mixin`
**Import Path:** `odoo.addons.binary_downloader.models.binary_utils`

A utility mixin providing common file operations.

#### `_download_and_extract()`
`COMM_binary_utils_download`

Safely downloads and extracts binaries from raw URLs, tarballs, or ZIP archives.

#### `_get_target_filename()`
`COMM_binary_utils_filename`

Generates a secure, version-aware filename for the downloaded binary.

#### `_unlink_binary_file()`
`COMM_binary_utils_unlink`

Safely removes a binary file from the filesystem.

#### `_is_ssrf_safe_public_ip()`
`binary_utils_is_ssrf_safe_public_ip`

Says whether an `ipaddress` object is an address a server-side download may connect to: true only for a public one, false for loopback, private-range, link-local (including the cloud metadata address 169.254.169.254), multicast, reserved and unspecified addresses. It gives the verdict of the shared `zero_sudo.daemon.ssrf_safe_fetch` predicate, which `pager_duty` also uses.

#### `_assert_host_is_ssrf_safe()`
`binary_utils_assert_host_is_ssrf_safe`

Resolves a hostname and raises `UserError` ("Security Alert") unless every address it resolves to is safe. `_download_and_extract()` calls it for the URL as given and again for wherever a redirect lands, so a download cannot be steered at an internal address.

#### `_safe_response_geturl()`
`binary_utils_safe_response_geturl`

Returns `response.geturl()` for a response object that has one, else `None`; the single place that tolerates `urlopen()`-like objects without a `geturl` method.

#### `_atomic_write_target()`
`binary_utils_atomic_write_target`

Writes a downloaded byte stream to its final path without a partly-written file ever being visible there, so an interrupted install cannot leave a truncated executable that later runs.
</api>

<usage>
## 3. Usage Example
```python
# To be called by other modules needing a binary dependency
bin_path = self.env["binary.manifest"].ensure_executable("kopia")
# Verified by COMM_test_binary_manifest_standard

# Verified by COMM_test_binary_manifest_integration
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
- `COMM_binary_ensure_executable` - Core binary resolution method.

- `COMM_binary_compute_installed` - Installation status computation.

- `COMM_binary_action_install` - UI installation trigger.

- `binary_downloader:UX_BINARY_INSTALL` - UI elements for installation.

- `COMM_test_binary_manifest_standard` - Standard unit tests.

- `COMM_test_binary_manifest_integration` - Unmocked physical integration tests.

- `COMM_pure_python_symlink_engine` - Pure Python symlink engine.

- `COMM_binary_resolution` - Binary resolution tenant isolation.

- `test_binary_install_tour` - UI tour for binary installation.

- `test_binary_manifest_views` - View rendering tests.

- `binary_version_download_pool` - Versioned binary download.

- `test_binary_version_standard` - Standard unit tests for versioning.

- `test_tenant_link_form` - View rendering tests for tenant links.

- `test_binary_version_form` - View rendering tests for binary versions.

## 6. External Dependencies
* `python`: `[]`
</semantic_anchors>

## Function reference (ADR 0113)

### `[@ANCHOR: binary_utils_count_binary_file_references]` `BinaryDownloaderMixin._count_binary_file_references(keys)`

**Purpose.** A downloaded binary is stored in `hams_bin` under a file name made from both the command name and the checksum (`_get_target_filename`). Before a manifest or a version is deleted, `binary.manifest.unlink` and `binary.version.unlink` need to know whether another record still points at the same file, so the file is removed only when the last user goes.

| Argument | Type | Meaning |
| --- | --- | --- |
| `keys` | iterable of `(cmd_name, checksum)` | The files to count. A pair with an empty name or checksum is ignored. |
| returns | `collections.Counter` | For each pair, the number of `binary.manifest` and `binary.version` rows that map to that same file (a version's command name is its manifest's name). An empty `Counter` when no valid key was given. |

**Special concerns.** An `@api.model` method that reads and never writes: one search of `binary.manifest` and one of `binary.version` by checksum, with Odoo's default filter (archived rows are not counted) and under the caller's access rules; the two callers run it as the Downloader service user (`with_user(svc_uid)`) so the count covers every record, not only those the deleting user can see. Two rows with the same checksum but different command names point at different files and do not count for each other. The count is taken before the caller's own deletion, so the caller compares against its own references. Safe to retry.

**See also.** `test_unlink_does_not_remove_a_still_referenced_binary_file` and `test_unlink_ignores_a_differently_named_manifest_sharing_the_checksum` in `tests/test_binary_version.py`.

## Anchor reference

One entry per anchor, written from the code in `models/`, `views/` and `tests/` of this module.

### `COMM_binary_ensure_executable` [@ANCHOR: COMM_binary_ensure_executable]
Purpose: the public entry point other modules use to get a verified program. `binary.manifest.ensure_executable(cmd_name)` is `@api.private @api.model`: callable from server code, not over RPC.

| Argument | Type | Meaning |
|---|---|---|
| `cmd_name` | str | Binary name, for example `"kopia"`. Empty, containing `/` or a backslash, `.` or `..` raises `ValidationError`. |

Returns the absolute path (str) of the verified executable in `hams_bin`. It finds the manifest as the `user_binary_downloader_service` account: the current company's manifest first, then the global one (no company), never another company's. It does not consult the system `PATH`, so a same-named program elsewhere cannot shadow the verified copy (the `zero_sudo` wrapper `_ensure_executable` tries `PATH` first when a caller wants that). Errors: `UserError` when no manifest exists, or from `_download_and_extract` (unsupported platform, SSRF refusal, size cap, checksum mismatch, extraction failure). Side effects: may download and write a file, taking a PostgreSQL advisory lock for the length of the transaction. Blocking; the first call for a new binary can take as long as the download. Safe to retry. Tests: `test_01_already_installed` to `test_04_successful_download_and_checksum` and `test_18_path_shadow_prevention` in `tests/test_binary_manifest.py`.

### `COMM_binary_compute_installed` [@ANCHOR: COMM_binary_compute_installed]
Purpose: the `is_installed` column of the manifest list. `binary.manifest._compute_is_installed()` is `@api.depends("name", "checksum", "archive_type", "extract_member")`, non-stored, no arguments and no return value. For each record it sets `is_installed` to `False` without a name; `True` if `shutil.which(name)` finds a program of that name on the system `PATH`; otherwise `True` when the file `<data_dir>/hams_bin/<name>_<id>` exists and is executable.

Special concerns: the `PATH` rule means a program installed by the operating system counts as installed, although `ensure_executable` never uses it (that is why the Install button is hidden for it). It reads the file system on each evaluation and never writes or downloads. Test: `test_06_is_installed_compute` in `tests/test_binary_manifest.py`.

### `COMM_binary_action_install` [@ANCHOR: COMM_binary_action_install]
Purpose: the "Install Now" button on a manifest. `binary.manifest.action_install()` is a `@public_rpc` method and needs exactly one record (`ensure_one`). Access: raises `UserError` ("You do not have sufficient permissions to install binaries.") unless the caller is in the Binary Downloader Manager group or is an administrator (`env.is_admin()`).

Behaviour: calls `ensure_executable(self.name)`, which resolves the manifest (current company, then global), downloads and verifies the file into `<data_dir>/hams_bin/` if it is not already there. Returns a client action: a "Success" notification ("Installation of X completed successfully.") followed by a page reload. Errors from `ensure_executable` (`UserError`) are passed on. Blocking for the length of the download. Tests: `test_07_action_install` and `test_12_action_install_permissions` in `tests/test_binary_manifest.py`, UI tour `test_binary_install_tour`.

### `COMM_binary_unlink` [@ANCHOR: binary_downloader:binary_manifest_unlink]
Purpose: delete a manifest without breaking anything that still shares its file. `binary.manifest.unlink()` takes no arguments and returns `True`. It runs as the service account, builds the `(name, checksum)` key of each record being deleted, asks `_count_binary_file_references` how many manifest and version rows map to the same file, subtracts the rows being deleted in this same call, and calls `_unlink_binary_file` only for files nothing else references, then does the standard unlink.

Special concerns: the file name depends on both name and checksum, so two records with the same checksum but different names do not protect each other's files. Deleting several records that share one file in a single call still removes it. A removal failure is logged, not raised. Tests: `test_19_unlink_privilege_escalation`, `test_20_unlink_deletes_the_real_file_when_the_checksum_is_not_shared` and `test_21_unlink_batch_deletes_the_file_when_every_sharing_record_is_removed_together` in `tests/test_binary_manifest.py`.

### `COMM_tenant_link_apply` [@ANCHOR: binary_downloader:pure_python_symlink_engine]
Purpose: the pure-Python engine that gives each tenant its own link to a shared binary. `binary.tenant.link.apply_symlink()` is `@api.private` (not callable over RPC) and needs one record. It downloads the active version to the pool first when `is_downloaded` is false, returns `False` if `symlink_path` is not set, creates `<data_dir>/tenant_bins/site_<id>/` (mode `0o750`) if needed, leaves a link that already points at the right file alone (returns `True`), removes a wrong link or file at the path and creates `os.symlink(central_path, symlink_path)`. Returns `True` on success.

Special concerns: it changes the file system and may download; no shell is involved. Called by `create` and by `write` when `active_version_id` changes. Because tenants point at one shared file, upgrades never copy the payload. Test: `test_pure_python_symlink_engine` in `tests/test_binary_manifest_integration.py`.

### `COMM_tenant_link_upgrade` [@ANCHOR: binary_downloader:binary_tenant_link_action_upgrade_to_latest]
Purpose: the "1-Click Upgrade to Latest" button. `binary.tenant.link.action_upgrade_to_latest()` is `@public_rpc` and needs one record. It finds the `binary.version` of the same manifest with the newest `release_date` (ties go to the highest id).

Returns: `False` when the manifest has no versions; a client notification "Up to Date" (type info) when the tenant already uses the newest; otherwise it sets `active_version_id` (which triggers `write`, hence the download and re-symlink) and returns an "Upgrade Successful" notification with the version number and a page reload. Errors: `UserError` from the download. It does not check the caller's group beyond the model's access rules (write access to the link is needed). Tests: `test_action_upgrade_to_latest_repoints_to_the_newest_version` and `test_action_upgrade_to_latest_is_a_no_op_when_already_current` in `tests/test_binary_tenant_link.py`.

### `COMM_binary_version_path` [@ANCHOR: binary_downloader:binary_version_get_central_path]
Purpose: where a release lives in the shared pool. `binary.version._get_central_path()` takes no arguments, requires exactly one record (`ensure_one`) and returns the absolute path `<data_dir>/hams_bin/<name>_<id>` built from `_get_target_filename(manifest name, checksum)`; `data_dir` comes from the Odoo configuration, default `/var/lib/odoo`.

Special concerns: the pool is not a separate store; a release and a manifest with the same name and checksum map to the same file. The method only builds the path and does not check that the file exists. Used by `_compute_is_downloaded`, `apply_symlink` (the link target) and the download action. Test: `test_get_central_path` in `tests/test_binary_version.py`.

### `COMM_binary_version_download` [@ANCHOR: binary_downloader:binary_version_download_pool]
Purpose: put a release into the shared pool. `binary.version.action_download_to_pool()` is `@public_rpc`, needs one record and returns `True`. Access: raises `UserError` unless the caller is in the Binary Downloader Manager group or is an administrator (portal users can read global versions, so without this any of them could trigger a download).

Behaviour: takes a transaction-scoped advisory lock keyed on manifest name, version and checksum, creates `hams_bin` (mode `0o750`) if missing, then calls `_download_and_extract(manifest name, url, checksum, archive_type, extract_member)`, with all its checksum, SSRF and archive checks. Errors: `UserError` from those checks. Blocking for the length of the download. Test: `test_download_to_pool_raw` in `tests/test_binary_version.py`.

### `COMM_binary_version_notify` [@ANCHOR: binary_downloader:binary_version_action_notify_tenants]
Purpose: tell tenants that a newer release exists. `binary.version.action_notify_tenants()` is a `@public_rpc` button method; it requires one record (`ensure_one`) and no other arguments. Access: raises `UserError` unless the caller is in the Binary Downloader Manager group or is an administrator (portal users can read versions, so the check is essential: without it any reader could raise real on-call pages).

Behaviour: it pages through `binary.tenant.link` rows of the same manifest whose `active_version_id` is not this release (100 per page, by id), groups them by the website's company, and creates one `pager.incident` per link (source `binary_update`, severity `medium`, description "New binary version X is available for Y", with the website) as the service account, one company at a time; it commits after each page. Returns a client notification action ("Tenants notified via PagerDuty."). Side effects: real incidents in the `pager_duty` module, so repeating the action re-pages. Tests: `test_pager_integration_batching` in `tests/test_binary_manifest_integration.py` and `test_action_download_to_pool_and_notify_tenants_reject_an_unprivileged_caller` in `tests/test_binary_version.py`.

### `COMM_binary_utils_download` [@ANCHOR: binary_downloader:binary_utils_download_and_extract]
Purpose: the one download engine used for raw binaries, `.tar.gz` and `.zip` archives. `binary_downloader.mixin._download_and_extract(cmd_name, url, checksum, archive_type, extract_member=None)` is an `@api.model` method in `models/binary_utils.py`, called by `binary.manifest.ensure_executable` and `binary.version.action_download_to_pool`.

| Argument | Type | Meaning |
|---|---|---|
| `cmd_name` | str | Binary name; becomes the file name prefix. A `..` component is refused. |
| `url` | str | Source address; must start with `https://`. |
| `checksum` | str | Expected SHA-256 (hex) of the whole downloaded file or archive. |
| `archive_type` | str | `binary`, `tar.gz` or `zip`. |
| `extract_member` | str or None | Archive member to take; defaults to `cmd_name`. |

Returns the absolute path `<data_dir>/hams_bin/<name>_<16 hex digits of sha256("<name>_<checksum>")>` of the executable (mode `0o750`). Errors (all `UserError`): not Linux on x86_64, aarch64 or armv7l; non-https URL; unsafe host or redirect (see `binary_utils_assert_host_is_ssrf_safe`); more than 1 GiB downloaded; checksum mismatch; archive member missing or a link; network or archive failure. Locking: takes the transaction-scoped PostgreSQL advisory lock `pg_advisory_xact_lock` keyed on name and checksum, so concurrent workers install once; the lock lasts until the transaction ends. Blocking network I/O with a 15 second timeout per request (a failed HEAD is only logged). An existing raw file is re-hashed and replaced if wrong; an existing extracted file is trusted. Writes go through `_atomic_write_target`. Tests: `test_04_successful_download_and_checksum`, the tar and zip tests `test_10` to `test_17`, `test_23_download_exceeding_size_cap_is_rejected` in `tests/test_binary_manifest.py`, and `tests/test_binary_manifest_integration.py`.

### `COMM_binary_utils_filename` [@ANCHOR: binary_downloader:binary_utils_get_target_filename]
Purpose: the one place that decides the file name of an installed binary. `_get_target_filename(cmd_name, checksum)` is an `@api.model` method of `binary_downloader.mixin` taking the binary name and its SHA-256 checksum (both str) and returning `<cmd_name>_<id>`, where `<id>` is the first 16 hex digits of the SHA-256 of the text `<cmd_name>_<checksum>`.

Special concerns: pure computation, no file or database access, safe to call anywhere. The name is stable for a given name and checksum, and a new checksum gives a new file, so an upgrade never overwrites a running binary and two records with the same name and checksum share one file. Callers: `_compute_is_installed`, `_get_central_path`, `_unlink_binary_file`; `_download_and_extract` computes the same value inline. Test: `test_06_is_installed_compute` in `tests/test_binary_manifest.py`.

### `COMM_binary_utils_unlink` [@ANCHOR: binary_downloader:binary_utils_unlink_binary_file]
Purpose: delete an installed binary file. `_unlink_binary_file(cmd_name, checksum)` is an `@api.model` method of `binary_downloader.mixin`; both arguments are str. It computes the file name with `_get_target_filename`, resolves the real path under `<data_dir>/hams_bin` and refuses (`UserError`, "Path traversal attempt detected") a name with a `..` component or a resolved path that is outside `hams_bin`. If the file exists it is unlinked; an `OSError` is logged as a warning and not raised. Returns nothing.

Special concerns: it does not count references; the callers (`binary.manifest.unlink`, `binary.version.unlink`) must decide that first with `_count_binary_file_references`. It does not remove tenant symlinks, which would dangle. Test: `test_unlink_ignores_a_differently_named_manifest_sharing_the_checksum` and `test_unlink_does_not_remove_a_still_referenced_binary_file` in `tests/test_binary_version.py`.

### `binary_utils_is_ssrf_safe_public_ip` [@ANCHOR: binary_utils_is_ssrf_safe_public_ip]
Purpose: classify an address for the download code. `_is_ssrf_safe_public_ip(ip_obj)` is a module-level function in `models/binary_utils.py`; `ip_obj` is an `ipaddress` object. It returns `True` only for a public address and `False` for loopback, private-range, link-local (including the cloud metadata address 169.254.169.254), multicast, reserved and unspecified addresses. It simply returns the verdict of `is_ssrf_safe_public_ip` in `zero_sudo.daemon.ssrf_safe_fetch`, shared with the `pager_duty` module; it stays a real function so older callers and tests keep working.

Special concerns: pure, no network or database access. Resolution and the pinning of the real connection are in `zero_sudo`; see `binary_utils_assert_host_is_ssrf_safe`.

### `binary_utils_assert_host_is_ssrf_safe` [@ANCHOR: binary_utils_assert_host_is_ssrf_safe]
Purpose: refuse a download host that resolves to a non-public address. `_assert_host_is_ssrf_safe(hostname, cmd_name)` is an `@api.model` method of `binary_downloader.mixin`; `hostname` (str) is the host to check and `cmd_name` (str) names the binary for the error message. It resolves the host with `resolve_ssrf_safe_addresses` from `zero_sudo` and raises `UserError("Security Alert: ...")` when the name is empty, does not resolve, or any address is unsafe; otherwise it returns nothing.

Special concerns: it performs a DNS lookup (blocking). It runs before the first request and again for wherever a redirect lands. It does not by itself stop DNS rebinding: the connection is made by `_urlopen_ssrf_safe`, which pins the socket to the validated address. It is a separate method so tests can patch it instead of needing live DNS. Tests: `test_rejects_a_direct_loopback_link_local_or_private_host` and the redirect tests in `tests/test_binary_manifest.py`.

### `binary_utils_safe_response_geturl` [@ANCHOR: binary_utils_safe_response_geturl]
Purpose: read the final URL of a response when it has one. `_safe_response_geturl(response)` is a module-level function in `models/binary_utils.py`; `response` is any `urlopen()`-like object. It returns `response.geturl()` when the object has that method and `None` otherwise, so the redirect check in `_download_and_extract` works with test doubles (plain byte streams) that lack `geturl`. No side effects. It is the single place in the module that uses this feature test, which the project linter allows by file and function name. Test: `test_safe_response_geturl_returns_none_for_an_object_without_geturl`.

### `binary_utils_atomic_write_target` [@ANCHOR: binary_utils_atomic_write_target]
Purpose: write the installed binary so that a half-written file never sits at its final path. `_atomic_write_target(source_fileobj, target_bin, bin_dir)` is an `@api.model` method of `binary_downloader.mixin`.

| Argument | Type | Meaning |
|---|---|---|
| `source_fileobj` | binary file object | Bytes to write (the raw download or one archive member). |
| `target_bin` | str | Final path inside `bin_dir`. |
| `bin_dir` | str | Directory for the temporary file, which must be on the same file system. |

It copies the bytes to a private temporary file in `bin_dir` and then `os.replace`s it onto `target_bin`, which is an atomic rename. On any error (including interruption) the temporary file is removed and the exception is re-raised. Returns nothing. Because extracted archive members are trusted on later calls (only the whole archive has a stored checksum), the guarantee that `target_bin` exists only after a complete write is what makes that trust safe. The final mode is set by the caller (`0o750`). Test: `test_22_interrupted_archive_extraction_never_leaves_a_partial_file_at_the_final_path` in `tests/test_binary_manifest.py`.

### `binary_downloader:UX_BINARY_INSTALL` [@ANCHOR: binary_downloader:UX_BINARY_INSTALL]
Purpose: marks the user-interface element that starts an install. In `views/binary_manifest_views.xml` the form `view_binary_downloader_manifest_form` has a header with the button "Install Now" (`name="action_install"`, `type="object"`, class `oe_highlight`, accessible label "Install Binary Now"). It is hidden when the record is already installed or not yet saved (`invisible="is_installed or not id"`) and only offered to the groups `binary_downloader.group_binary_downloader_manager` and `base.group_system`.

Special concerns: hiding the button is not the security control; the check lives in `action_install` itself. The element has no code of its own; the behaviour is that of `action_install`. Tests: `test_binary_install_tour` (clicks the button), `test_binary_manifest_views`.

### `COMM_test_binary_manifest_standard` [@ANCHOR: COMM_test_binary_manifest_standard]
Test anchor for the standard (mocked) unit tests of the manifest in `tests/test_binary_manifest.py`, starting at `test_01_already_installed`. The tests mock the network (`urlopen`), `shutil.which` and the platform checks and replace `_assert_host_is_ssrf_safe`, so no live DNS or download is needed. They cover an already installed binary (`test_01`), a missing manifest (`test_02`), an unsupported platform (`test_03`), a successful download with checksum (`test_04`), `action_install` and its permissions (`test_07`, `test_12`), path traversal (`test_08`), tar and zip slip and link refusal (`test_10` to `test_17`), PATH shadowing (`test_18`), safe unlink (`test_19` to `test_21`), interrupted extraction (`test_22`) and the size cap (`test_23`). It is the evidence for `ensure_executable`, `_compute_is_installed` and `action_install`. Run with `test.py -u binary_downloader`.

### `COMM_test_binary_manifest_integration` [@ANCHOR: COMM_test_binary_manifest_integration]
Test anchor for the unmocked integration tests in `tests/test_binary_manifest_integration.py`, class `TestBinaryManifestIntegration` (tags `post_install`, `-at_install`, `integration`). `test_kopia_install_integration` installs a binary through the real `ensure_executable` path into a real `hams_bin`; `test_pure_python_symlink_engine` checks the tenant symlink creation and re-pointing; `test_pager_integration_batching` checks that `action_notify_tenants` creates incidents in batches. Only the DNS-based host check is patched out so the suite does not depend on live DNS. Run with `test.py -u binary_downloader`; it writes under the Odoo data directory.

### `COMM_pure_python_symlink_engine` [@ANCHOR: COMM_pure_python_symlink_engine]
Second anchor on the same function as `COMM_tenant_link_apply`. Purpose: the pure-Python engine that gives each tenant its own link to a shared binary. `binary.tenant.link.apply_symlink()` is `@api.private` (not callable over RPC) and needs one record. It downloads the active version to the pool first when `is_downloaded` is false, returns `False` if `symlink_path` is not set, creates `<data_dir>/tenant_bins/site_<id>/` (mode `0o750`) if needed, leaves a link that already points at the right file alone (returns `True`), removes a wrong link or file at the path and creates `os.symlink(central_path, symlink_path)`. Returns `True` on success.

Special concerns: it changes the file system and may download; no shell is involved. Called by `create` and by `write` when `active_version_id` changes. Because tenants point at one shared file, upgrades never copy the payload. Test: `test_pure_python_symlink_engine` in `tests/test_binary_manifest_integration.py`.

### `COMM_binary_resolution` [@ANCHOR: COMM_binary_resolution]
Purpose: the tenant-aware resolution rule of `ensure_executable`, tagged on that function as "binary resolution tenant isolation". The manifest is searched with name equal to the argument and `company_id` in the current company or empty, with limit 2; the current company's record wins, then the global one. Another company's manifest is never returned, so one company cannot make another run its binary. The search runs as `user_binary_downloader_service`, whose only group is Binary Downloader Manager.

Special concerns: the rule applies per call, so switching company changes the result; the file name includes the checksum, so two companies with different manifests of the same name get different files in the shared `hams_bin`. See `binary_ensure_executable` for the full call interface. Test: `test_01_already_installed` in `tests/test_binary_manifest.py`.

### `test_binary_install_tour` [@ANCHOR: test_binary_install_tour]
Test anchor for the browser tour `TestBinaryDownloaderTour` in `tests/test_ui_tours_api.py` (class derived from `HamsHttpCase`, tags `post_install`, `-at_install`). The tour opens the Binary Manifests menu, creates a manifest named `tourbin` with a placeholder host and clicks "Install Now", which exercises `UX_BINARY_INSTALL` and `action_install`. The setup sets the admin language to US English and replaces `_assert_host_is_ssrf_safe`; teardown deletes the `tourbin_*` file the install writes, so later runs start clean. It needs a headless browser, so it is slower than the unit tests.

### `test_binary_manifest_views` [@ANCHOR: test_binary_manifest_views]
Test anchor for the view-rendering test of the manifest: `test_05_views_render` in `tests/test_binary_manifest.py` loads the list and form views with `get_view` and asserts that the `name` and `url` fields are present. The anchor is also cited in `views/binary_manifest_views.xml` as the test that covers both views (`view_binary_downloader_manifest_list` and `view_binary_downloader_manifest_form`). It checks that the XML compiles and carries the fields, not the permissions of the Install button.

### `binary_version_download_pool` [@ANCHOR: binary_version_download_pool]
Second anchor on the same function as `COMM_binary_version_download`. Purpose: put a release into the shared pool. `binary.version.action_download_to_pool()` is `@public_rpc`, needs one record and returns `True`. Access: raises `UserError` unless the caller is in the Binary Downloader Manager group or is an administrator (portal users can read global versions, so without this any of them could trigger a download).

Behaviour: takes a transaction-scoped advisory lock keyed on manifest name, version and checksum, creates `hams_bin` (mode `0o750`) if missing, then calls `_download_and_extract(manifest name, url, checksum, archive_type, extract_member)`, with all its checksum, SSRF and archive checks. Errors: `UserError` from those checks. Blocking for the length of the download. Test: `test_download_to_pool_raw` in `tests/test_binary_version.py`.

### `test_binary_version_standard` [@ANCHOR: test_binary_version_standard]
Test anchor for the unit tests of `binary.version` in `tests/test_binary_version.py`. `test_version_constraints` checks the three constraints (https only, no slash or dot name, extract member required); other tests there cover the unlink reference counting (`test_unlink_does_not_remove_a_still_referenced_binary_file` and a test that a differently named manifest sharing the checksum does not protect the file) and the download-to-pool action. Download and host checks are mocked. Run with `test.py -u binary_downloader`.

### `test_tenant_link_form` [@ANCHOR: test_tenant_link_form]
Test anchor for the tenant link form view: `test_05_2_tenant_link_views_render` in `tests/test_binary_manifest.py` loads the `binary.tenant.link` form with `get_view` and asserts that the computed field `symlink_path` appears in it. The view lives in `views/tenant_binary_views.xml`, which cites this anchor. It checks that the view renders, not that the symlink works; that is tested in `tests/test_binary_tenant_link.py`.

### `test_binary_version_form` [@ANCHOR: test_binary_version_form]
Test anchor for the binary version form view: `test_05_1_version_views_render` in `tests/test_binary_manifest.py` loads the `binary.version` form with `get_view` and asserts that `version_number` appears. The view is in `views/tenant_binary_views.xml`, which cites this anchor. It checks that the XML compiles and carries the field; it does not exercise the download or notify buttons.
