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

- **Real Dummy Daemon** `[@ANCHOR: zero_sudo:dummy_daemon_do_head]`: `tests/dummy_daemon.py` is a real, tiny standalone `HEAD`-only HTTP server started as a subprocess by the daemon-lifecycle integration test above, so the health-check machinery is proven against a genuine process, not a mock.

## Base Class Lifecycle, In Detail
- **HamsTransactionCase Setup** `[@ANCHOR: zero_sudo:hams_transaction_case_setup]`: Flushes the distributed Redis cache at the start of every test using this base class, so a leftover key from a prior test can never leak into the next one's assertions.

- **HamsTransactionCase Teardown** `[@ANCHOR: zero_sudo:hams_transaction_case_teardown_class]`: Stops the crypto-secret patchers and forcibly terminates any daemon subprocess this test class itself started (escalating SIGTERM to SIGKILL if needed), once per class, after its last test finishes.

- **Global BaseCase Teardown Patch** `[@ANCHOR: zero_sudo:patched_basecase_teardown]`: Replaces `BaseCase.tearDown` itself (Odoo's own ultimate test-class ancestor), so cache-flushing and local-cache clearing happen on the teardown of literally every test in the whole process, not only ones using this module's own base classes.

- **HamsHttpCase Setup** `[@ANCHOR: zero_sudo:hams_http_case_setup]` **/ Browser Startup** `[@ANCHOR: zero_sudo:hams_http_case_start_hams_browser]`: Every HTTP-based test spins up a real headless Chrome instance during `setUp`, whether or not that particular test ever navigates anywhere -- the browser is always available if a test needs it.

- **HamsHttpCase Teardown** `[@ANCHOR: zero_sudo:hams_http_case_teardown]` **/ Teardown Class** `[@ANCHOR: zero_sudo:hams_http_case_teardown_class]`: Per-test and per-class Chrome/proxy cleanup, symmetric with the setup above.

- **url_open Wrapper** `[@ANCHOR: zero_sudo:hams_http_case_url_open]`: A thin wrapper around core Odoo's own `url_open` that strips a stray `verify` kwarg and supports a `head=True` shortcut for HEAD requests -- used by essentially every controller-route test in both repos.

- **Screenshot Helper** `[@ANCHOR: zero_sudo:hams_http_case_navigate_and_screenshot]`: Navigates to a real URL in the managed Chrome instance and saves a screenshot, used by UI-persona-style tests that need a visual record rather than a tour's own pass/fail assertions.

- **Tour Runner** `[@ANCHOR: zero_sudo:hams_http_case_start_tour]` **/ browser_js Wrapper** `[@ANCHOR: zero_sudo:hams_http_case_browser_js]`: `start_tour` optionally forces a `debug=` query param (via `HAMS_TOUR_TOUR_DEBUG`) before delegating to core Odoo, which in turn calls this class's own `browser_js` override to actually drive the tour through the DevTools protocol.

- **Global HttpCase.browser_js Patch** `[@ANCHOR: zero_sudo:patched_browser_js]`: A SEPARATE, module-level monkeypatch applied directly to core Odoo's `HttpCase.browser_js` (not `HamsHttpCase`'s own override above) -- reached specifically by `RealTransactionCase`, which extends `HttpCase` directly rather than `HamsHttpCase`, so it has no subclass override of its own to shadow the patched parent method.

## Chrome/DevTools Protocol Monkeypatches
A cluster of module-level patches make headless-Chrome tour testing reliable in this sandboxed environment, each addressing one real, previously-observed failure mode:
- **Fetch Interception** `[@ANCHOR: zero_sudo:patched_handle_request_paused]`: Lets HTTPS loopback traffic through during tests rather than blocking it as an untrusted external fetch.

- **Process Group Isolation** `[@ANCHOR: zero_sudo:patched_preexec]`: Puts the spawned Chrome process in its own session/process group, so killing the test runner doesn't leave orphaned Chrome processes behind.

- **Chrome Spawn** `[@ANCHOR: zero_sudo:patched_spawn_chrome]`: Kills any stale headless-Chrome processes already owned by the current user before starting a fresh one, preventing port/profile collisions between test runs.

- **Werkzeug Request Thread** `[@ANCHOR: zero_sudo:patched_process_request_thread]`: Tracks each request-handling thread so the test harness can wait for them to drain before tearing down (see `wait_for_werkzeug_threads` below).

- **Screenshot Saving** `[@ANCHOR: zero_sudo:patched_save_test_file]`: Where a tour's own failure-screenshot gets written to disk.

- **Opener Init** `[@ANCHOR: zero_sudo:patched_opener_init]`, **Chrome Init** `[@ANCHOR: zero_sudo:patched_chrome_init]`, **Chrome Stop** `[@ANCHOR: zero_sudo:patched_chrome_stop]`, **Wait-Ready** `[@ANCHOR: zero_sudo:patched_wait_ready]`, **Chrome Start** `[@ANCHOR: zero_sudo:patched_chrome_start]`: The rest of the Chrome lifecycle -- constructing the CDP connection, waiting for it to actually be ready before the first navigation, and tearing it back down -- each patched for a real, previously-hit reliability issue specific to this sandboxed CI environment.

- **Draining Background Requests** `[@ANCHOR: zero_sudo:wait_for_werkzeug_threads]`: `RealTransactionCase`'s own teardown calls this before dropping its raw cursor, so a daemon's in-flight RPC request can't outlive the test block and cause a `SerializationFailure` on the way out.

## Safe Mocking
- **safe_patch** `[@ANCHOR: zero_sudo:safe_patch]` **/ safe_patch_object** `[@ANCHOR: zero_sudo:safe_patch_object]`: Thin wrappers around `unittest.mock.patch`/`patch.object` that auto-register `addCleanup(patcher.stop)` (so a forgotten `.stop()` can never leak a patch into a later test) and, unless the caller passes its own `new`/`new_callable`, default to `DiagnosticMock` instead of a bare `MagicMock`.

- **DiagnosticMock** `[@ANCHOR: zero_sudo:diagnostic_mock_init]` **/ Recursion Guard** `[@ANCHOR: zero_sudo:diagnostic_mock_call]`: A `MagicMock` subclass that trips a `RecursionError` past a configurable depth (default 5) -- catches a mock that's accidentally calling itself (a common copy-paste mistake when patching a method that recurses) loudly, rather than letting it silently spin or blow the real Python recursion limit.

- **safe_patch_object's Cursor Guard**: `safe_patch_object` refuses outright to mock anything whose type name contains `"Cursor"` -- mocking a database cursor corrupts the real test teardown sequence; `RealTransactionCase` is the documented alternative when a real side effect needs to be observed instead of mocked.

## Synthetic Callsigns
- **Callsign Generator** `[@ANCHOR: zero_sudo:generate_test_callsign]` **/ Cached Accessor** `[@ANCHOR: zero_sudo:get_callsign]`: Ham-radio test fixtures across both repos need a unique, valid-looking callsign per test without colliding on a real one -- `generate_test_callsign` hands out a monotonically-increasing synthetic value (`T0001X`, `T0002X`, ...), and `get_callsign(key)` caches one per class+key so repeated calls with the same key return the identical value within a test.

## Secure Daemon RPC
- **SecureJSONRPCClient Init** `[@ANCHOR: zero_sudo:json_rpc_client_init]` **/ Credential Loading** `[@ANCHOR: zero_sudo:json_rpc_client_load_credentials]`: External daemons authenticate to Odoo over JSON-2 IPC using credentials read strictly from a local `daemon_key_manager`-managed `.env` file (never a hardcoded secret) -- a missing file raises `FileNotFoundError`, a malformed one (missing login or key) raises `ValueError`, and a real key rotation mid-run triggers this client's own self-healing retry rather than a hard failure.
