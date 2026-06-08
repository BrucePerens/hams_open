# Jules VM Issues - Pager Duty

## Environment Hurdles

### 1. PostgreSQL Socket Disconnection
- **Issue:** PostgreSQL clusters intermittently shut down or close their Unix sockets during the `test.py` execution loop, particularly after the `infrastructure.py` smoketest.
- **Symptom:** `psycopg2.OperationalError: connection to server on socket "/var/run/postgresql/.s.PGSQL.5432" failed`.
- **Workaround:** Patched `tools/infrastructure.py` locally to prevent service teardown after smoketests when `is_test_env` is True. This must be backed out before PR submission.

### 2. UI Tour Asset Resolution (`@zero_sudo/js/tour_utils`)
- **Issue:** UI tours fail with `AssetNotFoundError` or `ChromeBrowserException` claiming `@zero_sudo/js/tour_utils` is missing, even when `zero_sudo` is a dependency and installed.
- **Symptom:** `AssertionError: The test code "odoo.startTour(...)" failed. The following modules are needed... but have not been defined`.
- **Root Cause:** In the Jules VM environment, the asset bundler seems to struggle with cross-module JS imports in `web.assets_tests` when running within a mount namespace. This results in the `TourUtils` export from `zero_sudo` not being correctly injected into the bundle for `pager_duty`.

## Framework Bugs / Constraints

### 1. Many2many Field Name Sensitivity
- Odoo 19 normalized Many2many user group fields to `group_ids`. Ensure no references to the legacy `groups_id` exist. Verified that `pager_duty` is compliant.

### 2. Multi-tenant Isolation
- Monitoring checks must strictly respect `website_id` for multi-tenant deployments. Added explicit indices and optimized queries to ensure data isolation is both secure and performant.

## Performance Optimizations

### 1. NOC Dashboard Postgres Procedure
- Implemented `pager_get_board_data` to fetch all NOC Dashboard statistics and incident lists in a single database round-trip. This significantly reduces latency when Odoo workers are physically distant from the database server.
- [@ANCHOR: pager_duty_postgres_procedures]
