# Story: Binary Resolution and Provisioning

## Purpose
The `ensure_executable` function is the core of the `binary_downloader` module. It ensures that a required binary is available on the system, downloading and verifying it if necessary.

> **Reading note:** the [module README](../../README.md) states that `binary.manifest.ensure_executable` deliberately does **not** consult the system `PATH` (so a same-named program elsewhere cannot shadow the verified copy) and supports Linux on x86_64, aarch64 or armv7l. Step 1 below, the `PATH` search, belongs to the `PATH`-first wrapper `zero_sudo.security.utils._ensure_executable` (per the README), and the x86_64-only check in step 3 is narrower than the README's platform list; where this story differs from the README, the README is the more detailed statement.

## Process
1. **Search System PATH:** It first checks if the binary is already available in the system's standard PATH using `shutil.which`.
2. **Lookup Manifest:** If not found, it switches to the `binary_downloader.user_binary_downloader_service` service account to search for a matching `binary.manifest` record.
3. **Architecture Check:** It ensures the current system is Linux x86_64 before attempting an auto-install.
4. **Local Cache Check:** It checks if the binary already exists in `the Odoo data directory (e.g., /var/lib/odoo/hams_bin/)`. If it does, it verifies the SHA-256 checksum.
5. **Download:** If the binary is missing or has a checksum mismatch, it downloads the file from the URL specified in the manifest to a temporary location.
6. **Checksum Verification:** The downloaded file's SHA-256 hash is compared against the manifest's expected checksum.
7. **Extraction:**
    - If it's a `tar.gz` archive, it extracts the specified member, implementing Tar Slip protection.
    - If it's a raw binary, it copies it to the target location.
8. **Permissions:** It sets the executable bits on the final binary.
9. **Return Path:** Returns the absolute path to the verified executable.

## Tenant Symlink Isolation
To support multi-tenancy, each tenant receives an isolated execution path. The system utilizes a pure Python symlink engine (`COMM_pure_python_symlink_engine`) to create OS-level symlinks pointing from the tenant's isolated directory to the central version pool. This ensures tenants can be upgraded independently without duplicating binary payloads on disk.

## Traceability
- **Code:** `ensure_executable` in `models/binary_manifest.py` `COMM_binary_ensure_executable`

- **Code:** `apply_symlink` in `models/binary_tenant_link.py` `COMM_pure_python_symlink_engine`

- **Anchor:** `COMM_binary_resolution`

## Anchor reference

One entry per anchor of this story, written from the code in `models/binary_manifest.py` and `models/binary_tenant_link.py`.

### `COMM_pure_python_symlink_engine` [@ANCHOR: COMM_pure_python_symlink_engine]
Purpose: the pure-Python engine that gives each tenant its own link to a shared binary. `binary.tenant.link.apply_symlink()` is `@api.private` (not callable over RPC) and needs one record. It downloads the active version to the pool first when `is_downloaded` is false, returns `False` if `symlink_path` is not set, creates `<data_dir>/tenant_bins/site_<id>/` (mode `0o750`) if needed, leaves a link that already points at the right file alone (returns `True`), removes a wrong link or file at the path and creates `os.symlink(central_path, symlink_path)`. Returns `True` on success.

Special concerns: it changes the file system and may download; no shell is involved. Called by `create` and by `write` when `active_version_id` changes. Because tenants point at one shared file, upgrades never copy the payload. Test: `test_pure_python_symlink_engine` in `tests/test_binary_manifest_integration.py`.

### `COMM_binary_ensure_executable` [@ANCHOR: COMM_binary_ensure_executable]
Purpose: the public entry point other modules use to get a verified program. `binary.manifest.ensure_executable(cmd_name)` is `@api.private @api.model`: callable from server code, not over RPC.

| Argument | Type | Meaning |
|---|---|---|
| `cmd_name` | str | Binary name, for example `"kopia"`. Empty, containing `/` or a backslash, `.` or `..` raises `ValidationError`. |

Returns the absolute path (str) of the verified executable in `hams_bin`. It finds the manifest as the `user_binary_downloader_service` account: the current company's manifest first, then the global one (no company), never another company's. It does not consult the system `PATH`, so a same-named program elsewhere cannot shadow the verified copy (the `zero_sudo` wrapper `_ensure_executable` tries `PATH` first when a caller wants that). Errors: `UserError` when no manifest exists, or from `_download_and_extract` (unsupported platform, SSRF refusal, size cap, checksum mismatch, extraction failure). Side effects: may download and write a file, taking a PostgreSQL advisory lock for the length of the transaction. Blocking; the first call for a new binary can take as long as the download. Safe to retry. Tests: `test_01_already_installed` to `test_04_successful_download_and_checksum` and `test_18_path_shadow_prevention` in `tests/test_binary_manifest.py`.

### `COMM_binary_resolution` [@ANCHOR: COMM_binary_resolution]
Purpose: the tenant-aware resolution rule of `ensure_executable`, tagged on that function as "binary resolution tenant isolation". The manifest is searched with name equal to the argument and `company_id` in the current company or empty, with limit 2; the current company's record wins, then the global one. Another company's manifest is never returned, so one company cannot make another run its binary. The search runs as `user_binary_downloader_service`, whose only group is Binary Downloader Manager.

Special concerns: the rule applies per call, so switching company changes the result; the file name includes the checksum, so two companies with different manifests of the same name get different files in the shared `hams_bin`. See `binary_ensure_executable` for the full call interface. Test: `test_01_already_installed` in `tests/test_binary_manifest.py`.
