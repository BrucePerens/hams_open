# Issues Encountered Running user_websites tests

During the testing process utilizing the `--provision-jules` command, the following issues occurred:

1. **Permission Denied for /var/lib/odoo/daemon_keys**:
   The initial database initialization and loading failed due to a missing directory and missing permissions:
   `PermissionError: [Errno 13] Permission denied: '/var/lib/odoo/daemon_keys'`

2. **Test Failures in Full Test Suite Execution**:
   When `--provision-jules` attempts to run the global test suite against all modules, there are a number of test failures:
   - `hams_test`: Cascade deletion failures from restricting foreign keys, e.g. `ERROR:  update or delete on table "res_company" violates RESTRICT setting of foreign key constraint "res_users_company_id_fkey" on table "res_users"`
   - `manual_library`: Cascade deletion constraints during the teardown of article tests `ERROR:  update or delete on table "knowledge_article" violates RESTRICT setting of foreign key constraint "knowledge_article_parent_id_fkey" on table "knowledge_article"`
   - `cloudflare`: API failure errors and test mocking failures. E.g. `ERROR: TestRequestContext.test_01_get_request_context` which encounters mock patching errors.
   - `user_websites`: Tests relating to exhaustive isolation result in Postgres unique constraint violations on `res_users_website_slug_unique`.

3. **Global Execution Timeouts**:
   The full database run against all modules encounters an execution timeout in the CI due to the large test suite and the extensive output of the tests (400 seconds).
