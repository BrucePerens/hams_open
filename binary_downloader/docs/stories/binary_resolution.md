# Story: Binary Resolution and Provisioning

## Purpose
The `ensure_executable` function is the core of the `binary_downloader` module. It ensures that a required binary is available on the system, downloading and verifying it if necessary.

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

## Traceability
- **Code:** `ensure_executable` in `models/binary_manifest.py` `[@ANCHOR: binary_ensure_executable]`
- **Anchor:** `[@ANCHOR: binary_resolution]`
