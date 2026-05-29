# Jules VM Testing Issues - distributed_redis_cache

## Standard Test Run Issues

- **Test skipped/failed due to Chrome startup error**: `TestDistributedCacheTour.test_distributed_cache_admin_tour`
  - **Error**: `Failed to detect chrome devtools port after 10.0s.`
  - **Chrome Error**: `[26211:26233:0529/005044.430086:ERROR:dbus/bus.cc:405] Failed to connect to the bus: Failed to connect to socket /dev/null: Connection refused`
  - **Context**: This occurred during the standard test run (`IN_JULES_VM=1 ./tools/test.py -u distributed_redis_cache --already-provisioned`). Interestingly, the test passed when run in integration mode.

## Provisioning Issues

- None. Provisioning completed successfully.
