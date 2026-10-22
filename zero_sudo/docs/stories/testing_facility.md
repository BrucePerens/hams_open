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
The [module README](../../README.md) attributes the end-of-test Database Leak Prevention checks to `RealTransactionCase` (`tests/real_transaction.py`). A table is exempt from the final comparison below when it is registered in `zero_sudo.noisy_table` (tables with high-frequency or technically necessary writes, such as `ir_logging` and `bus_bus`), and `RealTransactionCase` also keeps its own built-in exemption list.

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

- **Empty-Run Guard** `[@ANCHOR: zero_sudo:hoot_empty_run_detector_emit]`: A logging handler the `browser_js` wrapper attaches to the browser-console logger for the duration of each run, so that a hoot suite which executed NO tests fails instead of passing silently. Hoot reports an empty run as `Passed 0 tests` followed by `Test suite succeeded` -- and `Test suite succeeded` is exactly the signal `browser_js` waits for, so before this guard a wrapper over an empty suite passed every time while testing nothing. A deliberately empty run must say so by passing `expect_empty=True`; see the `linter-compliance` skill's own hoot rules for why deleting the wrapper is usually the better answer. Note this guard lives on `HamsHttpCase`'s override and therefore does NOT cover the separate module-level patch immediately below, which `RealTransactionCase` reaches instead.

- **Global HttpCase.browser_js Patch** `[@ANCHOR: zero_sudo:patched_browser_js]`: A SEPARATE, module-level monkeypatch applied directly to core Odoo's `HttpCase.browser_js` (not `HamsHttpCase`'s own override above) -- reached specifically by `RealTransactionCase`, which extends `HttpCase` directly rather than `HamsHttpCase`, so it has no subclass override of its own to shadow the patched parent method.

## Chrome/DevTools Protocol Monkeypatches
A cluster of module-level patches make headless-Chrome tour testing reliable in this sandboxed environment, each addressing one real, previously-observed failure mode:
- **Fetch Interception** `[@ANCHOR: zero_sudo:patched_handle_request_paused]`: Lets HTTPS loopback traffic through during tests rather than blocking it as an untrusted external fetch.

- **Process Group Isolation** `[@ANCHOR: zero_sudo:patched_preexec]`: Puts the spawned Chrome process in its own session/process group, so killing the test runner doesn't leave orphaned Chrome processes behind.

- **Chrome Spawn** `[@ANCHOR: zero_sudo:patched_spawn_chrome]`: Kills any stale headless-Chrome processes already owned by the current user before starting a fresh one, preventing port/profile collisions between test runs.

- **Werkzeug Request Thread** `[@ANCHOR: zero_sudo:patched_process_request_thread]`: Tracks each request-handling thread so the test harness can wait for them to drain before tearing down (see `wait_for_werkzeug_threads` below).

- **Screenshot Saving** `[@ANCHOR: zero_sudo:patched_save_test_file]`: Where a tour's own failure-screenshot gets written to disk.

- **Opener Init** `[@ANCHOR: zero_sudo:patched_opener_init]`, **Chrome Init** `[@ANCHOR: zero_sudo:patched_chrome_init]`, **Chrome Stop** `[@ANCHOR: zero_sudo:patched_chrome_stop]`, **Wait-Ready** `[@ANCHOR: zero_sudo:patched_wait_ready]`, **Chrome Start** `[@ANCHOR: zero_sudo:patched_chrome_start]`: The rest of the Chrome lifecycle -- constructing the CDP connection, waiting for it to actually be ready before the first navigation, and tearing it back down -- each patched for a real, previously-hit reliability issue specific to this sandboxed CI environment.

- **Retry Teardown** `[@ANCHOR: zero_sudo:tear_down_partly_built_browser]`: Core's `ChromeBrowser.__init__` has no error handling of its own, so a failed attempt (retried by the patched Chrome Init above) used to leave its partially-registered resources (profile directory, process, websocket, receiver thread) orphaned when the retry overwrote the same object's `cleanup` stack -- worse, the orphaned receiver thread's own teardown then ran against the SUCCEEDING attempt's healthy socket, silently breaking every CDP call for the rest of that browser's life. This closes out whatever the failed attempt actually registered before the retry starts from a clean object.

- **Draining Background Requests** `[@ANCHOR: zero_sudo:wait_for_werkzeug_threads]`: `RealTransactionCase`'s own teardown calls this before dropping its raw cursor, so a daemon's in-flight RPC request can't outlive the test block and cause a `SerializationFailure` on the way out.

- **Allowing Extra Fetch Hosts** `[@ANCHOR: zero_sudo:extra_allowed_fetch_hosts]`: The Fetch interception above lets a browser test reach only `HOST` (127.0.0.1) for real; every other host fails the request, so a test cannot make an unbounded real external request by accident. `HttpCase.extra_allowed_fetch_hosts` (default: an empty tuple) is the opt-in for a test that needs a real round trip to one other, hermetic host it controls, such as a fake relay served by an `HTTPServer` the same test starts and stops. The test sets it to a tuple of literal host names and the interception then lets those hosts through exactly as it does `HOST`. With nothing set, behaviour is unchanged.

- **Navigator Overrides** `[@ANCHOR: zero_sudo:install_navigator_overrides]`: Registers, through the DevTools protocol, a script that runs at the start of every document the test browser loads. It makes `navigator.onLine` read true and gives `navigator.virtualKeyboard` a harmless stub, both defined on `Navigator.prototype` and never on the `navigator` instance, because hoot's `createMock(navigator, ...)` breaks when the instance has own keys. Without it the sandbox's headless Chromium reports itself offline, which sent every tour's writes into the offline queue, and its VirtualKeyboard API crashed web's `dvu.js` at module load. A failure to register is logged and tolerated: the same script still runs from the `ready` step of `browser_js`, only later.

- **Reaping Leaked Headless Chrome** `[@ANCHOR: zero_sudo:kill_all_headless_chrome]`: Kills every headless `chrome`, `chromium` or `chromium-browser` process owned by the current uid, system-wide, not only this test's own children. It runs before a new Chrome is spawned and at the end of every test, so memory held by a tour that did not tear down completely is reclaimed at once. It is safe to be this aggressive because `test.py` allows only one run at a time on the box. A non-headless browser, another user's process, and any process that is not one of those three browsers are left alone.

## Keeping the Harness Honest
- **Transaction-Impedance Hint** `[@ANCHOR: zero_sudo:monitored_test_execute]`: Wraps `TestCursor.execute`. When PostgreSQL reports a serialization failure (40001), a lock that is not available (55P03) or a unique violation (23505) inside the isolated test cursor, it logs a critical message telling the developer that a background thread, daemon or raw SQL has bypassed the test's uncommitted savepoint and that the test class should inherit from `RealTransactionCase`, then re-raises the original error. Every other result and error passes through untouched, and the call signature is forwarded unchanged because Odoo core passes keyword arguments such as `log_exceptions`.

- **Tracked Background Jobs** `[@ANCHOR: zero_sudo:tracked_threadpool_submit]` **/ Waiting for Them** `[@ANCHOR: zero_sudo:wait_for_background_futures]`: `ThreadPoolExecutor.submit` is replaced by a version that records every future, and its callable's name, until the future completes. Production code defers some work to a thread pool after commit (for example the asynchronous unpublish in `user_websites`), and that work opens its own database connection, so it can still be writing a row while a `RealTransactionCase` teardown tries to delete it. `wait_for_background_futures(timeout)` waits for all tracked jobs. It is best effort: a job still running after the timeout is named in a warning and the function returns anyway, so a job that never finishes cannot hang a test run.

## Safe Mocking
- **safe_patch** `[@ANCHOR: zero_sudo:safe_patch]` **/ safe_patch_object** `[@ANCHOR: zero_sudo:safe_patch_object]`: Thin wrappers around `unittest.mock.patch`/`patch.object` that auto-register `addCleanup(patcher.stop)` (so a forgotten `.stop()` can never leak a patch into a later test) and, unless the caller passes its own `new`/`new_callable`, default to `DiagnosticMock` instead of a bare `MagicMock`.

- **safe_patch_object_cm** `[@ANCHOR: zero_sudo:safe_patch_object_cm]`: The same cursor guard and `DiagnosticMock` default as `safe_patch_object`, but it returns the unstarted patcher and registers no cleanup. It serves a patch that must end earlier than full test teardown, such as one that only needs to cover a single call driving a browser through several navigations in one profile. The caller owns the lifetime, normally by passing the result to `ExitStack.enter_context()` or using it in a `with` block. Raw `patch.object` stays forbidden in test files, and this is the sanctioned alternative.

- **DiagnosticMock** `[@ANCHOR: zero_sudo:diagnostic_mock_init]` **/ Recursion Guard** `[@ANCHOR: zero_sudo:diagnostic_mock_call]`: A `MagicMock` subclass that trips a `RecursionError` past a configurable depth (default 5) -- catches a mock that's accidentally calling itself (a common copy-paste mistake when patching a method that recurses) loudly, rather than letting it silently spin or blow the real Python recursion limit.

- **safe_patch_object's Cursor Guard**: `safe_patch_object` refuses outright to mock anything whose type name contains `"Cursor"` -- mocking a database cursor corrupts the real test teardown sequence; `RealTransactionCase` is the documented alternative when a real side effect needs to be observed instead of mocked.

## Synthetic Callsigns
- **Callsign Generator** `[@ANCHOR: zero_sudo:generate_test_callsign]` **/ Cached Accessor** `[@ANCHOR: zero_sudo:get_callsign]`: Ham-radio test fixtures across both repos need a unique, valid-looking callsign per test without colliding on a real one -- `generate_test_callsign` hands out a monotonically-increasing synthetic value (`T0001X`, `T0002X`, ...), and `get_callsign(key)` caches one per class+key so repeated calls with the same key return the identical value within a test.

## Secure Daemon RPC
- **SecureJSONRPCClient Init** `[@ANCHOR: zero_sudo:json_rpc_client_init]` **/ Credential Loading** `[@ANCHOR: zero_sudo:json_rpc_client_load_credentials]`: External daemons authenticate to Odoo over JSON-2 IPC using credentials read strictly from a local `daemon_key_manager`-managed `.env` file (never a hardcoded secret) -- a missing file raises `FileNotFoundError`, a malformed one (missing login or key) raises `ValueError`, and a real key rotation mid-run triggers this client's own self-healing retry rather than a hard failure.
