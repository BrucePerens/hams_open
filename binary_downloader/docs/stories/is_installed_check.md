# Story: Installation Status Tracking

## Purpose
The `_compute_is_installed` method dynamically tracks whether a binary is currently available and executable on the host system.

## Process
1. **System Path Check:** It uses `shutil.which` to see if the binary name is found in the standard system PATH.
2. **Local Path Check:** It checks the module's dedicated binary directory (`the Odoo data directory (e.g., /var/lib/odoo/hams_bin/)`) for the presence of the executable file.
3. **Validation:** It verifies that the file exists and has the execution bit set (`os.X_OK`).
4. **State Update:** Updates the `is_installed` boolean field on the `binary.manifest` record.

## Traceability
- **Code:** `_compute_is_installed` in `models/binary_manifest.py` `COMM_binary_compute_installed`

## Anchor reference

The one anchor of this story, written from `models/binary_manifest.py`.

### `COMM_binary_compute_installed` [@ANCHOR: COMM_binary_compute_installed]
Purpose: the `is_installed` column of the manifest list. `binary.manifest._compute_is_installed()` is `@api.depends("name", "checksum", "archive_type", "extract_member")`, non-stored, no arguments and no return value. For each record it sets `is_installed` to `False` without a name; `True` if `shutil.which(name)` finds a program of that name on the system `PATH`; otherwise `True` when the file `<data_dir>/hams_bin/<name>_<id>` exists and is executable.

Special concerns: the `PATH` rule means a program installed by the operating system counts as installed, although `ensure_executable` never uses it (that is why the Install button is hidden for it). It reads the file system on each evaluation and never writes or downloads. Test: `test_06_is_installed_compute` in `tests/test_binary_manifest.py`.
