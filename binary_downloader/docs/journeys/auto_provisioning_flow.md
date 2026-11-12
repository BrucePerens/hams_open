# Journey: Automated Binary Provisioning

## Overview
This journey describes how an Odoo subsystem (like `backup_management`) automatically acquires a required external binary (like `kopia`) without manual intervention.

## Actors
- **Calling Module:** An Odoo module that depends on an external binary.
- **Binary Downloader:** The orchestration module managing the lifecycle of the binary.
- **Service Account:** `user_binary_downloader_service` which performs the secure operations.

## The Journey

### 1. The Request
The calling module invokes the API:
```python
bin_path = self.env["binary.manifest"].ensure_executable("kopia")
```
- **Anchor:** `COMM_binary_ensure_executable`

### 2. Resolution
Binary Downloader checks the system. If `kopia` is not in the PATH, it looks up the `binary.manifest` record. (The [module README](../../README.md) states that `ensure_executable` itself never consults the `PATH`; the `PATH`-first step belongs to the wrapper `zero_sudo.security.utils._ensure_executable`, which the README says calls `ensure_executable` only when a service account is given.)
- **Logic:** `ensure_executable` in `models/binary_manifest.py`.

### 3. Verification & Download
If the local cache in `the Odoo data directory (e.g., /var/lib/odoo/hams_bin/)` is empty or invalid (checksum mismatch):
1. Downloads the binary using `urllib.request`.
2. Verifies SHA-256 integrity.
3. Extracts if it's a tarball, applying Tar Slip protection.
- **Security:** Handled by the dedicated service account.

### 4. Execution
The calling module receives the absolute path and executes the binary.
```python
subprocess.run([bin_path, "--version"], check=True, shell=False)
```

## Summary
This journey ensures that external dependencies are managed securely, predictably, and automatically, fulfilling the requirement for DB-backed manifests and cryptographic verification.

## Anchor reference

The one anchor of this journey, written from `models/binary_manifest.py`.

### `COMM_binary_ensure_executable` [@ANCHOR: COMM_binary_ensure_executable]
Purpose: the public entry point other modules use to get a verified program. `binary.manifest.ensure_executable(cmd_name)` is `@api.private @api.model`: callable from server code, not over RPC.

| Argument | Type | Meaning |
|---|---|---|
| `cmd_name` | str | Binary name, for example `"kopia"`. Empty, containing `/` or a backslash, `.` or `..` raises `ValidationError`. |

Returns the absolute path (str) of the verified executable in `hams_bin`. It finds the manifest as the `user_binary_downloader_service` account: the current company's manifest first, then the global one (no company), never another company's. It does not consult the system `PATH`, so a same-named program elsewhere cannot shadow the verified copy (the `zero_sudo` wrapper `_ensure_executable` tries `PATH` first when a caller wants that). Errors: `UserError` when no manifest exists, or from `_download_and_extract` (unsupported platform, SSRF refusal, size cap, checksum mismatch, extraction failure). Side effects: may download and write a file, taking a PostgreSQL advisory lock for the length of the transaction. Blocking; the first call for a new binary can take as long as the download. Safe to retry. Tests: `test_01_already_installed` to `test_04_successful_download_and_checksum` and `test_18_path_shadow_prevention` in `tests/test_binary_manifest.py`.
