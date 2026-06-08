# Jules Issues - binary_downloader

## Hurdles & Findings
- **PostgreSQL Socket Location:** The PostgreSQL Unix socket was not found in `/var/run/postgresql` during initial testing. It was resolved by manually starting PostgreSQL with the correct configuration.
- **`tools/test.py` Bug:** A syntax error in `tools/test.py`'s `FailureExtractor.finish_and_write` was identified and patched (`AttributeError: 'list' object has no attribute 'extendgrouped_blocks'`).
- **Archive Extraction Security:**
    - `binary.manifest.py` and `binary.version.py` use `tarfile.extract` and `zip_ref.open` which require careful path validation.
    - Current implementation has basic path traversal checks (Zip Slip/Tar Slip), but could be more robust.
- **Performance:**
    - `action_notify_tenants` in `pager_integration.py` performs a search on `binary.tenant.link` without indices on `manifest_id` and `active_version_id`.
- **UI/UX:**
    - The module relies on `hams_bin` which is a shared directory. Versioning is handled by appending a hash to the filename, which is good for isolation.

## Suggestions
- Add indices to `binary.tenant.link` for `manifest_id` and `active_version_id`.
- Strengthen archive extraction security by strictly using `os.path.basename` for all extracted files in the `hams_bin` directory, avoiding any directory structure from the archive.
- Ensure `hams_bin` and `tenant_bins` are properly cleaned up if a record is deleted (already partially handled for symlinks in `unlink`).
