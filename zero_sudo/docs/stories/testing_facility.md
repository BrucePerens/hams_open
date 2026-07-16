<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: Real Transaction Testing Facility

The `zero_sudo` module provides a comprehensive testing framework that enables true transaction testing and isolated daemon integration.

## Base Test Classes
- **HamsTransactionCase** `[@ANCHOR: zero_sudo:COMM_hams_transaction_case]`: The foundational class for tests requiring raw transaction context.
The testing facility provides a safe environment.


- **HamsHttpCase** `[@ANCHOR: zero_sudo:COMM_hams_http_case]`: Extended class for running UI tours and HTTP cases under true transactional isolation.

## The Leak Detection Mechanism
- **Cursor Hijacking** `[@ANCHOR: zero_sudo:COMM_cursor_hijacking]`: The facility intercepts the test cursor to provision a real, committable PostgreSQL connection.
The testing facility provides a safe environment.


- **Leak Snapshotting** `[@ANCHOR: zero_sudo:COMM_leak_snapshotting]`: Prior to the test, the system records the exact sizes of all active database tables.
The testing facility provides a safe environment.


- **ORM Instrumentation** `[@ANCHOR: zero_sudo:COMM_orm_instrumentation]`: The framework actively intercepts and tracks all records created natively via the ORM.
The testing facility provides a safe environment.


- **Automated Cleanup** `[@ANCHOR: zero_sudo:COMM_automated_cleanup]`: A multi-pass algorithm attempts to cleanly unlink all tracked ORM records at the end of the test execution.
The testing facility provides a safe environment.


- **Leak Verification** `[@ANCHOR: zero_sudo:COMM_leak_verification]`: The system compares the final table sizes against the initial snapshots. Any discrepancies (indicative of raw SQL inserts bypassing the ORM) instantly fail the test.

## Integration Services
- **Real Transaction Service** `[@ANCHOR: zero_sudo:COMM_user_real_transaction_service]`: Ensures background workers operate against unmocked, durable database states.
The testing facility provides a safe environment.


- **Integration Daemon Testing** `[@ANCHOR: zero_sudo:COMM_integration_daemon_testing]`: Supports spinning up and health-checking isolated Python daemons alongside the Odoo test suite to guarantee end-to-end integration safety.
