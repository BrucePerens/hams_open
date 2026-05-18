# Story: Installation Status Tracking

## Purpose
The `_compute_is_installed` method dynamically tracks whether a binary is currently available and executable on the host system.

## Process
1. **System Path Check:** It uses `shutil.which` to see if the binary name is found in the standard system PATH.
2. **Local Path Check:** It checks the module's dedicated binary directory (`the Odoo data directory (e.g., /var/lib/odoo/hams_bin/)`) for the presence of the executable file.
3. **Validation:** It verifies that the file exists and has the execution bit set (`os.X_OK`).
4. **State Update:** Updates the `is_installed` boolean field on the `binary.manifest` record.

## Traceability
- **Code:** `_compute_is_installed` in `models/binary_manifest.py` `[@ANCHOR: binary_compute_installed]`
