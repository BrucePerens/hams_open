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
To support multi-tenancy, each tenant receives an isolated execution path. The system utilizes a pure Python symlink engine (`[@ANCHOR: COMM_pure_python_symlink_engine]`) to create OS-level symlinks pointing from the tenant's isolated directory to the central version pool. This ensures tenants can be upgraded independently without duplicating binary payloads on disk.

## Traceability
- **Code:** `ensure_executable` in `models/binary_manifest.py` `[@ANCHOR: COMM_binary_ensure_executable]`

- **Code:** `apply_symlink` in `models/binary_tenant_link.py` `[@ANCHOR: COMM_pure_python_symlink_engine]`

- **Anchor:** `[@ANCHOR: COMM_binary_resolution]`
