# JULES_ISSUES.md - distributed_redis_cache

## VM Environment Hurdles
- [2026-06-08] PostgreSQL and Redis services were found inactive upon start. Manual intervention via `systemctl start` was required.
- [2026-06-08] `tools/test.py` Lifecycle Failures: The test runner's internal provisioning logic (via `infrastructure.py`) frequently fails to manage systemd services correctly in this VM. Specifically, it attempts to restart PostgreSQL and Redis, which often results in "Unit is masked" or "Job failed" errors, blocking the entire test suite.
- [2026-06-08] `hams_com` Cloning: The `test.py` script attempts to clone `hams_com` from GitHub, which fails due to lack of terminal prompts/credentials in the VM. This causes the test runner to report failures early in the process.
- [2026-06-08] Database Rebuild Race Conditions: `createdb` often fails with "database already exists" because the `dropdb` command preceding it fails to connect to the socket, or the socket is not yet ready after a service restart triggered by the runner.

## Resolution Attempted
- Manually restarted services and confirmed connectivity with `pg_isready` and `redis-cli ping`.
- Verified module logic through targeted linter runs and manual source review.
- Semantic Anchors and Documentation gaps (ADR-0054, ADR-0055) have been fully resolved.

## External Module Dependencies
- **zero_sudo/models/security_utils.py**: The following parameters must be whitelisted in `_get_param_whitelist()` to support secure Redis configuration via UI:
  - `distributed_redis_cache.redis_host`
  - `distributed_redis_cache.redis_port`
  - `distributed_redis_cache.redis_password`
  - `distributed_redis_cache.test_integration_active`
