# Jules Session Issues - zero_sudo

## Environment Hurdles
- **Chrome Headless Connectivity:** Initial tour runs failed due to Chrome being unable to connect to the DBus. This was resolved by ensuring `google-chrome-stable` was fully up to date and correctly configured in the VM environment.
- **Permission Denied on /opt/hams:** The test runner initially failed because it lacked permissions to create directories in `/opt/hams/pgdata`. Ownership was manually adjusted during the session to allow PostgreSQL to initialize.

## External Module Bugs
- **pager_duty manifest incompleteness:** The `pager_duty` module had a critical manifest error (missing `views/board_templates.xml` in the `data` array) that blocked all linters and tests. A temporary local fix was applied to allow `zero_sudo` processing to proceed.

## Test Logic Improvements
- **Controller Test vs Password Randomization:** The `test_01_web_login_interceptor` was failing because `res.users.create` automatically randomizes passwords for service accounts. The test was modified to use `RealTransactionCase` and commit a manual UPDATE to bypass the randomization for testing purposes.
