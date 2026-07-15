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
- **Anchor:** `[@ANCHOR: COMM_binary_ensure_executable]`

### 2. Resolution
Binary Downloader checks the system. If `kopia` is not in the PATH, it looks up the `binary.manifest` record.
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
