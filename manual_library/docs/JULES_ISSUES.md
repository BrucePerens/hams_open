# Jules Issues for manual_library

## Provisioning Issues
- Documentation in `docs/TESTING_IN_JULES.md` refers to `tools/test_runner.py`, but the script is actually located at `tools/test.py`.

## Test Running Issues
- `TestManualORMLogic.test_04_parent_deletion_restriction` failed with `psycopg2.errors.RestrictViolation`.
  - Error: `update or delete on table "knowledge_article" violates RESTRICT setting of foreign key constraint "knowledge_article_parent_id_fkey" on table "knowledge_article"`
- `TestManualLibraryUITours.test_01_manual_toc_tour` failed or hung.
  - Log: `=== TOUR FAILED OR HUNG. DUMPING COMPILED ASSETS ===`
