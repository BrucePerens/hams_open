# Real Transaction Testing Facility Stories

## Automated Database Cleanup
As a developer, I want my test data to be automatically removed after each test run, even when using real database commits, so that the test environment remains pristine.

The testing facility achieves this by instrumenting the ORM's creation method ([@ANCHOR: orm_instrumentation]) to track every record created during the test. When the test finishes, it executes an automated cleanup process ([@ANCHOR: automated_cleanup]) that unlinks these records in multiple passes to handle foreign key constraints.

## SQL Leak Detection
As a quality assurance engineer, I want to be alerted if any test leaves behind "orphaned" data that bypasses the ORM, so that I can ensure database integrity.

The facility takes a mathematical snapshot of all table row counts before the test begins ([@ANCHOR: leak_snapshotting]). After the ORM cleanup, it performs a leak verification ([@ANCHOR: leak_verification]) by comparing current row counts against the initial snapshot. If any discrepancies are found (excluding known "noisy" system tables), the test fails with an informative error.

## Base Testing Classes
The custom environment introduces new base classes. Developers should inherit from `hams_transaction_case` ([@ANCHOR: hams_transaction_case]) for pure ORM operations, and `hams_http_case` ([@ANCHOR: hams_http_case]) for controller tests.

## Production-Realistic Testing
As a backend developer, I want to test features like cross-worker cache invalidation and complex inverse relationships that require actual database commits, so that I can find bugs that standard Odoo tests might miss.

By performing a cursor hijacking ([@ANCHOR: cursor_hijacking]), the facility replaces Odoo's standard test cursor with a real, committable PostgreSQL connection. This allows developers to call `self.env.cr.commit()` and observe how the system behaves when data is actually written to disk.

## Noisy Table Management
As a developer, I want to be able to mark certain tables as "noisy" through the UI, so that I can easily exclude them from leak detection without changing the codebase.

The system provides a dedicated interface for managing noisy tables ([@ANCHOR: UX_NOISY_TABLE_MANAGEMENT]). This allows administrators to add table names that should be ignored during the leak verification phase. This UI functionality is verified by a tour ([@ANCHOR: test_noisy_table_tour]).
