# Jules Session Issues - binary_downloader

## Environment Verification
- **Date:** 2026-05-30
- **Provisioning:** Successful using `IN_JULES_VM=1 python3 tools/test.py --provision-jules -u binary_downloader`.
- **Standard Tests:** Passed.
- **UI Tours:** Passed.
- **Linter/Anchor Issues:** The anchor linter reports duplicates because it scans both the root module directory and the `hams_community/` directory created during provisioning. This is a known environment hurdle and does not represent actual duplicates in the module itself.

## AI Hallucination & Laziness Audit
- **Empty Exception Handlers:** Verified that `models/binary_manifest.py`, `tests/test_binary_manifest.py`, and `tests/test_ui_tours_api.py` have proper logging in exception blocks.
- **UI Visibility Logic:** Checked `views/binary_manifest_views.xml`. The `extract_member` field is correctly visible for both `tar.gz` and `zip` archives.
- **UI Tour Robustness:** `static/tests/tours/binary_install_tour.js` uses `TourUtils.waitForAbsence('.o_loading')` and includes success notification checks.

## Multi-Tenant Awareness
- Binaries are system-wide resources stored in `hams_bin`, but versioned via name+checksum hash to allow multi-tenant coexistence.
- The `binary.manifest` model now includes `company_id` for strict ownership and isolation.

## Security Audit
- Verified SHA-256 integrity checks, advisory locking, and path sanitization (Tar/Zip slip protection).
- **Symlink Protection:** Added explicit check for symlinks in ZIP archives (via `external_attr`) to match Tarball security.
- **Zero-Sudo:** No `.sudo()` calls found; strictly uses `with_user()` with service accounts.
