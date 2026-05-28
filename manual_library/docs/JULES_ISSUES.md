# Jules VM Testing Issues - manual_library

## Provisioning Issues

- `ERROR: role "jules" already exists` and `ERROR: role "odoo" already exists` during PostgreSQL role creation. These are non-fatal as the provisioning process completed successfully.

## Test Failures

### ORM Tests
- **ERROR**: `TestManualORMLogic.test_04_parent_deletion_restriction`
    - **Issue**: The test expects `ForeignKeyViolation` (from `psycopg2.errors`), but Odoo/PostgreSQL 18 raises `psycopg2.errors.RestrictViolation`.
    - **Traceback**:
      ```python
      Traceback (most recent call last):
        File "/app/manual_library/tests/test_orm_logic.py", line 78, in test_04_parent_deletion_restriction
          self.article_a.unlink()
        ...
      psycopg2.errors.RestrictViolation: update or delete on table "knowledge_article" violates RESTRICT setting of foreign key constraint "knowledge_article_parent_id_fkey" on table "knowledge_article"
      DETAIL:  Key (id)=(89) is referenced from table "knowledge_article".
      ```

### UI Tours
- **SKIPPED**: `TestManualLibraryUITours.test_01_manual_toc_tour`
    - **Issue**: Failed to start Chrome headless in the Jules VM environment.
    - **Error**: `Failed to detect chrome devtools port after 10.0s.`
    - **Root Cause**: Likely related to DBus connection failures within the containerized/VM environment: `Failed to connect to the bus: Could not parse server address`.
    - **Note**: Other tours (`test_02_manual_search_tour`, `test_03_manual_feedback_tour`) succeeded in the same run, suggesting an intermittent issue or something specific to the first tour run.

## General Observations
- The test runner reported "1 error(s) of 32 tests" and then a summary "3 test failure(s) detected!". The discrepancy between Odoo's internal count and the runner's count might be due to how skips and non-standard errors are handled.
