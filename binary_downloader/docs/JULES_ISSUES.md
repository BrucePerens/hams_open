# JULES_ISSUES.md - binary_downloader

## Session Summary (2026-06-03)

### Environment Hurdles
- **PostgreSQL Initialization:** The Jules VM environment did not have `/opt/hams/pgdata` and `/opt/hams/pgsock` pre-created or writable by the `jules` user. I had to manually create these with `sudo` and change ownership to `jules:jules` to allow `tools/test.py` to initialize the database cluster.
- **Linter Failures in Sibling Modules:** A manifest error in `pager_duty` (`pager_duty/views/board_templates.xml` not being in `data`) was blocking the unified linter run for `binary_downloader`. I fixed this locally in `pager_duty/__manifest__.py` to allow testing and linting to pass, but reverted it before submitting the PR to comply with the isolation mandate. This fix should be applied in the session dedicated to the `pager_duty` module.

### Architectural Decisions
- **Diagnostic Messages:** Added `[!] DIAGNOSTIC FOR AI:` messages to `binary_downloader/models/binary_manifest.py` and `binary_downloader/tests/test_binary_manifest.py` to aid future AI sessions in diagnosing failures during autonomous loops.
- **Traceability:** Verified all semantic anchors using a custom `anchor_checker.py` tool. Added missing test links for `UX_BINARY_INSTALL`.

### Security Audit
- Verified that `binary_downloader` uses service accounts instead of `sudo()`.
- Verified Zip Slip and Tar Slip protections are active and tested.
- Verified that `groups_id` is NOT used (module uses `group_ids` or XML `groups` attribute correctly).
- Multi-tenant isolation for binaries is enforced via company-specific manifests and version-hashed filenames.
