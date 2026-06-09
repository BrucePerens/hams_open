# JULES ISSUES - Database Management

## Environment Hurdles
- **Infrastructure Flakiness:** The Jules VM environment occasionally fails to maintain the PostgreSQL socket connection during the database rebuild phase of `tools/test.py`. Manual service restarts are sometimes required to restore connectivity.
- **Cross-Module Asset Loading:** UI Tours in this module consistently failed to load the `@zero_sudo/js/tour_utils` library, resulting in "module not found" errors despite correct manifest dependencies. (Resolved by using standard Odoo tour syntax and waiting steps).

## Module Specific Actions
- **Performance Optimization:** Optimized `vacuumdb` execution and session termination to batch operations, reducing database round-trips from N to 1. Session termination now uses a PostgreSQL `DO` block.
- **Security Hardening:** Strictly enforced minimal environment variables for subprocess calls and added missing ACLs for replication statistics.
- **Tour Robustness:** Refactored tours to use robust Odoo 19/Owl waiting patterns and added a dedicated `TestTours` class for execution.
- **Python Tests:** Enhanced test suite to cover new performance optimizations and security constraints using `PropertyMock` for readonly view fields.
