# Hams Test Infrastructure (`hams_test`)

<system_role>
You are an expert Odoo architect and testing engineer. This module provides the core testing infrastructure for the entire repository, including Real Transaction testing, Integration Daemon management, and UI Tour standards.
</system_role>

<architecture>
The module implements three primary testing facilities:

1.  **Real Transaction Testing (`RealTransactionCase`)**: Bypasses Odoo's standard `TestCursor` to provide a real, committable database connection. It uses ORM instrumentation ([@ANCHOR: orm_instrumentation]) and mathematical table snapshots ([@ANCHOR: leak_snapshotting]) to ensure database integrity.
2.  **Integration Daemon Testing (`HamsIntegrationCase`)**: Provides a lifecycle management wrapper for external Python daemons, including automated health polling.
3.  **UI Tour Governance**: Defines standards for JavaScript-based UI tours and provides `TourUtils` for robust frontend testing.

It also includes a **Noisy Table Management** interface ([@ANCHOR: UX_NOISY_TABLE_MANAGEMENT]) to allow administrators to whitelist tables from leak detection.
</architecture>

<security_design>
- **Service Accounts**: Utilizes `user_real_transaction_service` ([@ANCHOR: user_real_transaction_service]) for background tasks and documentation injection.
- **Zero-Sudo Compliance**: Strictly avoids `.sudo()` by leveraging the `zero_sudo` security utilities and micro-privilege service accounts.
- **SQL Injection Prevention**: Uses `psycopg2.sql.Identifier` and `psycopg2.sql.Literal` for all dynamic SQL in the leak detector ([@ANCHOR: leak_verification]).
</security_design>

<stories_and_journeys>
### Stories
- [Real Transaction Testing](hams_test/docs/stories/real_transaction_testing.md): Explains the need for real commits and how the facility handles them ([@ANCHOR: cursor_hijacking]).
- [Documentation Injection](hams_test/docs/stories/documentation_injection.md): Describes the automated documentation setup process ([@ANCHOR: documentation_bootstrap]).

### Journeys
- [Developer Testing Flow](hams_test/docs/journeys/developer_testing_flow.md): Guides developers through using `RealTransactionCase` for advanced integration tests.
- [Documentation Setup Flow](hams_test/docs/journeys/documentation_setup_flow.md): Details the technical steps of injecting documentation into the knowledge base.
</stories_and_journeys>
