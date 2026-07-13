# Story: Real Transaction Testing Facility

The `zero_sudo` module provides a comprehensive testing framework that enables true transaction testing and isolated daemon integration.

## Base Test Classes
- **HamsTransactionCase** `[@ANCHOR: COMM_hams_transaction_case]`: The foundational class for tests requiring raw transaction context.
- **HamsHttpCase** `[@ANCHOR: COMM_hams_http_case]`: Extended class for running UI tours and HTTP cases under true transactional isolation.

## The Leak Detection Mechanism
- **Cursor Hijacking** `[@ANCHOR: COMM_cursor_hijacking]`: The facility intercepts the test cursor to provision a real, committable PostgreSQL connection.
- **Leak Snapshotting** `[@ANCHOR: COMM_leak_snapshotting]`: Prior to the test, the system records the exact sizes of all active database tables.
- **ORM Instrumentation** `[@ANCHOR: COMM_orm_instrumentation]`: The framework actively intercepts and tracks all records created natively via the ORM.
- **Automated Cleanup** `[@ANCHOR: COMM_automated_cleanup]`: A multi-pass algorithm attempts to cleanly unlink all tracked ORM records at the end of the test execution.
- **Leak Verification** `[@ANCHOR: COMM_leak_verification]`: The system compares the final table sizes against the initial snapshots. Any discrepancies (indicative of raw SQL inserts bypassing the ORM) instantly fail the test.

## Integration Services
- **Real Transaction Service** `[@ANCHOR: COMM_user_real_transaction_service]`: Ensures background workers operate against unmocked, durable database states.
- **Integration Daemon Testing** `[@ANCHOR: COMM_integration_daemon_testing]`: Supports spinning up and health-checking isolated Python daemons alongside the Odoo test suite to guarantee end-to-end integration safety.
