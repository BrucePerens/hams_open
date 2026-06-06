# JULES_ISSUES.md - zero_sudo

## Environment Hurdles

### Chrome DevTools Port Detection Failure
The UI tour `test_02_zero_sudo_tour` failed with the error: `skipped TestZeroSudoViews.test_02_zero_sudo_tour : Failed to detect chrome devtools port after 10.0s.`
This appears to be an environment-specific issue in the Jules VM where headless Chrome is failing to bind or report its DevTools port correctly, despite suppression of other common headless errors.
Wait times and retries in `zero_sudo/tests/common.py` did not resolve this during the session.

### PostgreSQL Service Initialization
The `tools/test.py` script failed initially because the PostgreSQL service was not running or reachable via the default socket. Manual intervention was required to start the service:
`sudo -u postgres /usr/lib/postgresql/18/bin/pg_ctl -D /etc/postgresql/18/main/ start`
Future Jules sessions should ensure the database service is verified before running tests.

## Missing Resources
- `knowledge.article` and `manual.article` models were not present in the test environment, causing documentation bootstrap tests to be skipped. These modules likely depend on additional apps not installed in the minimal test run.
