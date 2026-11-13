<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: Real Transaction Testing Facility

The `zero_sudo` module provides a comprehensive testing framework that enables true transaction testing and isolated daemon integration.

## Base Test Classes
### HamsTransactionCase `[@ANCHOR: zero_sudo:COMM_hams_transaction_case]`

**Purpose.** The base class for module tests that need the ORM in an isolated transaction plus this project's helpers. Defined in `zero_sudo/tests/common.py` as `class HamsTransactionCase(TransactionCase, SafePatchMixin, PersonaMixin)`: Odoo's `TransactionCase` (every test method in a savepoint that is rolled back; a `commit()` only releases a savepoint), the safe-mock helpers (`safe_patch`, `safe_patch_object`) and the persona helpers (`@personas`, `assert_persona_access`). Use it for tests that stay inside one process and one cursor; use `RealTransactionCase` when a thread, daemon or second connection has to see committed data.

**Class data.** `_active_daemons` (list of `subprocess.Popen`, default `[]`): the daemons started by `start_daemon`. `setUpClass` gives each class its own list, because the class-body list is shared by every subclass that does not shadow it.

**Added behaviour.** `setUpClass` makes a test Fernet key, patches the secret readers, writes the frozen `web.base.url` rows and clears caches; `setUp` flushes the Redis cache; `start_daemon` starts a real daemon process; `tearDownClass` stops them. Each is documented in its own entry below.

**Errors and side effects.** Nothing is raised by the class itself. It touches the database (config parameter rows, inside the class transaction), the process environment (`HAMS_CRYPTO_KEY` if unset) and Redis (`flushall` per test). Not thread-safe: Odoo runs the tests of one process one after another.

**See also.** Tests `zero_sudo/tests/test_facility.py`, `test_harness_helpers.py`.

### HamsHttpCase `[@ANCHOR: zero_sudo:COMM_hams_http_case]`

**Purpose.** The base class for HTTP, tour (UI) and browser tests: `class HamsHttpCase(HttpCase, SafePatchMixin, PersonaMixin)` in `zero_sudo/tests/common.py`. It keeps Odoo's `HttpCase` isolation (the server thread and the test share one `TestCursor` over the class's savepoint transaction, so nothing a test writes survives it) and adds this project's setup and teardown, a TLS (Transport Layer Security) proxy, the Chrome pool and the browser patches described below.

**Class data.** `_hams_tour_failed` (bool, default `False`; set by `start_tour` when a tour raises, and read by `tearDown` to decide whether to clear V8 hang logs), `server_thread`, `server`, `browser` (the test's `ChromeBrowser`, `None` until a test asks for one), `_socat_proc` (the HTTPS proxy process of the class), and the two class-level defaults `http_request_key = ""` and `http_request_allow_all = False`, which exist so that a health poll arriving between `current_test` being set and the first `setUp` does not crash Odoo's cursor check with an `AttributeError`.

**Special concerns.** It starts a real `socat` process on port 8443 per class and a headless Chrome on demand, so it needs those binaries and the `/opt/hams/test` directories; nothing else is mocked. A tour or browser test that fails leaves nothing behind: teardown kills the browser tree and reaps headless Chrome of this run.

**See also.** `HamsHttpCase Class Set-Up`, `Setup`, `Teardown` and `Tour Runner` below; tests `test_facility.py`, `test_views.py`.

### invalidate_zero_sudo_utils_cache(test_class) `[@ANCHOR: zero_sudo:invalidate_zero_sudo_utils_cache]`

Called at the end of `HamsTransactionCase.setUpClass` and `HamsHttpCase.setUpClass`, after the class has written its `ir_config_parameter` rows, to drop the cached `zero_sudo.security.utils` entries. Takes the test class (its `registry` and `env`) and returns nothing. When the registry has no `zero_sudo.security.utils` model it does nothing: a module that deliberately does not depend on zero_sudo (`edge_cache`, which a Redis-less tenant instance installs) still uses these base classes, and its own run (`-u edge_cache`) used to end in a `KeyError` in `setUpClass` because Redis settings are read through that model.

## The Leak Detection Mechanism
The [module README](../../README.md) attributes the end-of-test Database Leak Prevention checks to `RealTransactionCase` (`tests/real_transaction.py`). A table is exempt from the final comparison below when it is registered in `zero_sudo.noisy_table` (tables with high-frequency or technically necessary writes, such as `ir_logging` and `bus_bus`), and `RealTransactionCase` also keeps its own built-in exemption list.

### Cursor Hijacking `[@ANCHOR: zero_sudo:COMM_cursor_hijacking]`

**Purpose.** `RealTransactionCase.setUp` (in `zero_sudo/tests/real_transaction.py`) replaces the savepoint cursor that Odoo's test harness hands out with a real, committable PostgreSQL connection, so that a test behaves like production: its `commit()` is a real `COMMIT` that background threads, daemons and other connections can see.

**How.** `_real_cursor_factory(readonly=False)` returns `odoo.sql_db.db_connect(registry.db_name).cursor()`. If `registry.cursor` is a `unittest.mock.Mock` (the harness installs one under `--test-enable`), its `side_effect` is swapped for the factory, not its attribute deleted, so Werkzeug threads do not deadlock and mock teardown still works; otherwise `registry.cursor` is replaced by the factory. A cleanup restores the original side effect or attribute. `self.cr` is then a real cursor and `self.env` an environment for the `admin` user (id 2 when not found), not the banned superuser id.

**Errors and concurrency.** None raised here. Each test owns one real connection, closed in `_real_teardown` with a rollback first. The original `HttpCase` test cursor is kept in `_test_cursor` and restored last so Odoo's own teardown works.

**See also.** Test `COMM_test_cursor_hijacking` in `test_facility.py`.

### Leak Snapshotting `[@ANCHOR: zero_sudo:COMM_leak_snapshotting]`

**Purpose.** Before the test body runs, `RealTransactionCase.setUp` records the exact row count of every table so that the teardown can prove the test left nothing behind.

**How.** It selects every table name in `information_schema.tables` for schema `public` except `pg_stat_statements*` into `self._tables`, then calls `count_rows_in_batches` and keeps the result in `self._initial_counts` (`dict[str, int]`; `{}` when there are no tables). `self._tracked_records` (a `defaultdict(set)` of model name to ids) is created at the same time.

**Cost and limits.** A count of every table (several hundred) runs at the start of each test, in batches. Views are included in `information_schema.tables`, which is why the statistics views are in `LEAK_CHECK_FALLBACK_TABLES`. Counts are exact, so any other writer on the same database while the test runs shows up as a leak.

**See also.** Test `COMM_test_leak_snapshotting`; `Leak Verification` below compares against this snapshot.

### count_rows_in_batches `[@ANCHOR: zero_sudo:COMM_count_rows_in_batches]`

**Signature.** `count_rows_in_batches(db_name, tables, batch_size=COUNT_BATCH_SIZE, on_batch=None) -> dict[str, int]` in `zero_sudo/tests/real_transaction.py`.

| Argument | Type | Meaning |
|---|---|---|
| `db_name` | `str` | Database to count in. |
| `tables` | iterable of `str` | Table or view names (safely quoted with `sql.Identifier`). |
| `batch_size` | `int` | Tables per transaction; default `COUNT_BATCH_SIZE` = 8. |
| `on_batch` | callable or `None` | Called with the cursor after each batch's query and before its transaction ends (the test uses it to read `pg_locks`). |

**Returns.** `{table: exact count}` for every table given. **Why.** One `UNION ALL` over all public tables (about 776 tables, 3,700 relations with indexes) inside one transaction held all those relation locks until it ended and exhausted Postgres's shared lock table (`max_locks_per_transaction=64`) when several test shards ran together ("out of shared memory"). Here each batch of eight runs on one short-lived connection and is rolled back, which releases its locks before the next batch. **Errors.** Database errors propagate (the cursor is closed in a `finally`). A table that does not exist raises. No writes. **See also.** Test `COMM_test_count_rows_in_batches`.

### ORM Instrumentation `[@ANCHOR: zero_sudo:COMM_orm_instrumentation]`

**Purpose.** Records every record the test creates through the ORM, so that teardown can delete them and distinguish them from raw-SQL inserts.

**How.** `RealTransactionCase.setUp` replaces `odoo.models.BaseModel.create` with `tracking_create(model_self, *args, **kwargs)`, which calls the saved original (`_original_create`, captured once at import to avoid descriptor binding problems), and for a non-empty result adds the new ids to `self._tracked_records[model_self._name]`. A cleanup restores the original `create` after the test. The tracking covers every model and every connection in the process, including records created by server threads during the test.

**Limits and security.** Only `create` is tracked: rows written by raw SQL, `INSERT ... SELECT`, or by a separate process are not, and are what `Leak Verification` reports. The patch is process-wide while a test runs, so a `RealTransactionCase` test must not run concurrently with another test in the same process (Odoo does not).

**See also.** Test `COMM_test_orm_instrumentation`; `Automated Cleanup`.

### Automated Cleanup `[@ANCHOR: zero_sudo:COMM_automated_cleanup]`

**Purpose.** At the end of a `RealTransactionCase` test, removes every record the test created, since its commits are real and nothing rolls them back.

**Algorithm (`_real_teardown`).** First it waits for Werkzeug request threads (`wait_for_werkzeug_threads`, 5 s) and tracked thread-pool jobs (`wait_for_background_futures`, 10 s), then rolls back the cursor. It makes up to 5 passes over the tracked models in reverse creation order: in a savepoint, with `odoo.sql_db` and `odoo.models.unlink` logging muted, it `browse`s the ids with `active_test=False`, keeps those that still `exists()`, and `unlink`s them as user 2. Foreign-key blockers are retried on the next pass (so children go before parents). On the fifth failing attempt it logs and falls back to a raw `DELETE FROM <table> WHERE id IN (...)` in its own savepoint, so one model's failure cannot poison the rest. Handled errors: `IntegrityError`, `OperationalError`, `AccessError`, `UserError`, `RedirectWarning`, `ValidationError`; anything else is logged with a traceback and counts as pending. The result is committed.

**Special.** Runs on the test's real connection; never raises for a cleanup failure, which instead shows up as a leak in `Leak Verification`. **See also.** Test `COMM_test_automated_cleanup`.

### Leak Verification `[@ANCHOR: zero_sudo:COMM_leak_verification]`

**Purpose.** After cleanup, `_real_teardown` compares the final table counts with the snapshot and fails the test when they differ, which catches raw SQL inserts and cleanup failures.

**Algorithm.** It reads the active `zero_sudo.noisy_table` names (limit 1,000; a failure to read is logged and ignored), unions them with `LEAK_CHECK_FALLBACK_TABLES`, and counts the remaining snapshot tables with `count_rows_in_batches`. Every table whose final count differs from its initial count is reported as `name (+n)` or `name (-n)`.

**Errors.** Raises `AssertionError("Database pollution detected! Auto-cleanup failed or raw SQL was used. Leaked records: ...")` listing the leaks. In every case (`finally`) it then rolls back, clears the registry cache and environment, closes the real connection and restores the original `HttpCase` cursor, logging, not raising, any failure to close.

**Special.** A fall in a count (a test that deleted existing data) also fails. Exempt tables are the noisy ones that production code writes on its own; add a table to `zero_sudo.noisy_table` rather than weakening the check. **See also.** Test `COMM_test_leak_verification`, `test_facility.py`.

### Leak Check Fallback Tables `[@ANCHOR: zero_sudo:COMM_leak_check_fallback_tables]`

`LEAK_CHECK_FALLBACK_TABLES` in `zero_sudo/tests/real_transaction.py`, a module-level `frozenset[str]` of table and view names (no schema, no quoting) that the leak check in the teardown skips regardless of the `zero_sudo.noisy_table` records. Two kinds of name belong in it: tables Odoo itself writes during any test (`bus_bus`, `mail_*`, `ir_cron*`, `http_session`, `ir_config_parameter` and similar) and the `database_management` views over `pg_stat_*` (`database_table_stat`, `database_query_stat`, `database_activity`, `database_index_stat`, `database_index_advisor`, `database_replication_stat`, `database_pg_setting`), whose row count follows server statistics, not test writes. `database_index_advisor` lists tables with more than 100 sequential scans and over 10 MB, so any module's queries can add a row mid-test; that is what failed `cloudflare` `test_content_hook_multi_website` in the 66-module whole-system run (`Leaked records: database_index_advisor (+1)`) while it passed with cloudflare installed alone.
  - **Fields:** a set, so no fields; each element is a lowercase PostgreSQL relation name in schema `public`. Created at import time, immutable, shared read-only by every `RealTransactionCase`; nothing mutates it.
  - **Used by:** `RealTransactionCase._real_teardown`, which unions it with the active `zero_sudo.noisy_table` names before comparing final to initial row counts.
  - **Invariant:** every `public` view whose definition reads `pg_stat_*`, `pg_settings` or `pg_locks` is in the set. Add a new statistics view here in the same change that creates it.
  - **Verified by:** `zero_sudo:COMM_test_leak_check_fallback_tables` (`test_09_statistics_views_are_never_counted_as_leaks`), which queries `information_schema.views` for the views installed in the database and fails on any that is missing. Related: `zero_sudo:COMM_leak_verification`.

## Integration Services
### Real Transaction Service `[@ANCHOR: zero_sudo:COMM_user_real_transaction_service]`

**Purpose.** `RealTransactionCase` (`zero_sudo/tests/real_transaction.py`, `class RealTransactionCase(HttpCase, SafePatchMixin)`) is the test base class for code that depends on real commits: background workers, daemons, thread-pool jobs after commit, `pg_notify`, a browser on another connection. Its tests run against the unmocked, durable database, so the code under test behaves as in production, and the class then removes what the test created and fails if anything is left.

**Interface.** Subclass it; use `self.env` and `self.cr` (real, committable). Extra methods: `authenticate(user, password)`, which commits after sign-in (see its entry). `setUpClass` commits the frozen `web.base.url` rows on a separate connection (see its entry). Class-level `http_request_key` and `http_request_allow_all` defaults cover the pre-setUp window.

**Concerns.** Slower than a savepoint test (a full table-count snapshot and a cleanup pass per test), and it must tolerate other writers, so use it only where real commits matter. See the entries Cursor Hijacking, Leak Snapshotting, ORM Instrumentation, Automated Cleanup and Leak Verification for each step.

### Integration Daemon Testing `[@ANCHOR: zero_sudo:COMM_integration_daemon_testing]`

**Purpose.** Lets a test start a real standalone Python daemon next to the Odoo test, wait until it answers, and be sure it is stopped afterwards, so daemon-to-Odoo integration is tested against a genuine process and not a mock.

**Interface.** `HamsTransactionCase.start_daemon(script_path, args=None, env_vars=None, health_url=None, timeout=600)` (see its entry in the harness section) starts the process through `zero_sudo.daemon.utils._start_daemon_process` (its own session, so the whole group can be signalled), records it in the class's `_active_daemons`, and with `health_url` polls every 0.5 s until HTTP 200 or 204; `tearDownClass` sends SIGTERM to the process group, escalates to SIGKILL after 2 s, and clears the list.

**Errors.** `RuntimeError` when the daemon exits before it is healthy, `TimeoutError` when it never answers within `timeout` seconds. **Tests.** `test_integration.py` starts `dummy_daemon.py` and checks the lifecycle (`COMM_test_integration_daemon_testing`).

### Real Dummy Daemon `[@ANCHOR: zero_sudo:dummy_daemon_do_head]`

`Handler.do_HEAD(self)` in `zero_sudo/tests/dummy_daemon.py` answers every `HEAD` request with status 200 and no body (`send_response(200)`, `end_headers()`); it takes no arguments beyond the request handler's own and returns nothing. The file is a tiny standalone HTTP server (`socketserver.TCPServer` on `0.0.0.0` port 1234, `serve_forever`), started as a subprocess by the daemon-lifecycle integration test in `test_integration.py`, so the health-check machinery in `start_daemon` is proven against a genuine process, not a mock. It has no authentication and listens on all interfaces, which is acceptable only because it runs briefly on a test box; it does not run in production.

## Base Class Lifecycle, In Detail
### HamsTransactionCase Setup `[@ANCHOR: zero_sudo:hams_transaction_case_setup]`

`HamsTransactionCase.setUp(self)` runs Odoo's own `setUp`, then obtains the Redis connection through `get_redis_connection(self.env)` and calls `flushall()` on it when one is configured, so a leftover key from a prior test can never leak into the next one's assertions. Any exception from Redis is logged at warning level and swallowed: a Redis-less install (a tenant instance) runs the same tests. It has no arguments and returns nothing. The flush is global to the Redis server (`flushall`, not `flushdb`), which is safe only on a test box with no other Redis users. The global `BaseCase` teardown patch (below) flushes the database again afterwards.

### HamsTransactionCase Teardown `[@ANCHOR: zero_sudo:hams_transaction_case_teardown_class]`

`HamsTransactionCase.tearDownClass(cls)` runs once after the class's last test. It stops every daemon in `cls._active_daemons`: it sends SIGTERM to the daemon's whole process group (the daemon was started in its own session, so children it forked are included) and waits 2 s; if the daemon has not exited it sends SIGKILL to the group and then `kill()`s the process; an already-exited process is ignored and any other error is logged. Then it clears the list and calls the parent `tearDownClass`. It does not stop the crypto-secret patchers itself: they were registered with `startClassPatcher`, and unittest's class cleanup stops them, so a second `stop()` would raise. It signals only the daemons this class started.

### Global BaseCase Teardown Patch `[@ANCHOR: zero_sudo:patched_basecase_teardown]`

`_patched_basecase_teardown(self, *args, **kwargs)` replaces `BaseCase.tearDown` itself (Odoo's own ultimate test-class ancestor), so the clean-up runs on the teardown of literally every test in the process, not only ones using this module's own base classes. It calls `flushdb()` on the distributed Redis connection (`get_redis_connection(self.env)`), clears the in-process `_local_cache` of `distributed_redis_cache`, and then calls the saved original `tearDown`. A Redis or cache error is logged at info level and swallowed, so a Redis-less install still tears down. It takes no other arguments and returns nothing. It flushes the whole Redis database, which is safe only because test boxes do not share Redis with production.

### HamsHttpCase Setup `[@ANCHOR: zero_sudo:hams_http_case_setup]` / Browser Startup `[@ANCHOR: zero_sudo:hams_http_case_start_hams_browser]`

`setUp` starts no browser (until 2026-11-08 it started a headless Chrome for every test of every HTTP class, whether or not the test navigated anywhere, and a tour's `browser_js()` then built a second one whose spawn reaped the first). `self.browser` stays `None` until a test calls `start_hams_browser()` (or `navigate_and_screenshot()`, which does), which builds a `ChromeBrowser` and caches it on the test for the rest of that test. A tour needs no call: `browser_js()` builds its own browser.

### HamsHttpCase Teardown `[@ANCHOR: zero_sudo:hams_http_case_teardown]`

`HamsHttpCase.tearDown(self)` runs after each test. It calls the parent `tearDown` (tolerating the "socket is already closed", `WebSocketConnectionClosedException` and `BrokenPipeError` errors a closing browser causes, and logging others), then in a `finally`: sends one `Connection: close` request to `/odoo/health` and closes the HTTP opener; closes the browser's cleanup stack and, for a browser that owns its Chrome (not one leased from the pool, whose Chrome belongs to the pool), kills its process tree with `_kill_browser_tree`, then terminates and kills the process; neuters the receiver thread's `join`; truncates `v8_hang*.log` files under the log directory unless a tour failed; and finally reaps every headless Chrome of this run with `_kill_all_headless_chrome_for_this_uid`. Each step catches and logs its own errors so a later step still runs. Nothing is raised for cleanup failures.

### HamsHttpCase Teardown Class `[@ANCHOR: zero_sudo:hams_http_case_teardown_class]`

`HamsHttpCase.tearDownClass(cls)` runs once after the class. It closes the class's pooled Chrome (`_chrome_pool.close()`), stops the two crypto-secret patchers, then, holding the per-class lock file `/opt/hams/test/socat_certs/hams_test_proxy_<uid>_<class>.lock` exclusively, stops the `socat` TLS proxy of the class: SIGTERM to its process group (unless that group is ours), `terminate()`, a 2 s wait, then SIGKILL of the group and `kill()`; it closes the proxy's log file and releases the lock. Errors are logged, never raised, and `_socat_proc` is set to `None`. It never signals its own process group.

### url_open Wrapper `[@ANCHOR: zero_sudo:hams_http_case_url_open]`

**Signature.** `url_open(self, url, data=None, timeout=30, headers=None, allow_redirects=True, head=False, **kwargs) -> requests.Response`.

| Argument | Meaning |
|---|---|
| `url`, `data`, `headers`, `allow_redirects` | Passed to core `HttpCase.url_open` unchanged. |
| `timeout` | Seconds, default 30 (core's is 10): the first request after Odoo rebuilds its routing map waits for that rebuild (11.8 s was measured), and a slow answer is not a failure. A hang still is. |
| `head` | `True` sends a `HEAD` request (`method="HEAD"`). |
| `**kwargs` | Passed on, except `verify`, which is dropped (the opener's `verify` is already `False` for the self-signed proxy). |

**Returns** the response of core's `url_open`. **Errors** are core's (`requests` exceptions, timeouts). Used by essentially every controller-route test in both repositories. No state.

### Screenshot Helper `[@ANCHOR: zero_sudo:hams_http_case_navigate_and_screenshot]`

**Signature.** `navigate_and_screenshot(self, url_path, prefix="screenshot_")`; `url_path` is a path on the test server (appended to `base_url()`), `prefix` the screenshot file prefix. It starts the test's browser with `start_hams_browser()` if there is none, then, inside `allow_requests(browser=browser)` (so the test cursor is available to the server), navigates there waiting for the page to stop loading, sleeps 1 s, and takes a screenshot, waiting up to 10 s for the file to be written. A failure to take the screenshot is logged as a warning and does not fail the test. Returns nothing. Used by UI-persona tests that want a visual record rather than a tour's pass/fail. The file is saved through the patched `save_test_file`, so it only appears when `SAVE_LOGS=1`.

### Tour Runner `[@ANCHOR: zero_sudo:hams_http_case_start_tour]`

**Signature.** `start_tour(self, *args, **kwargs)`: takes the arguments of core `HttpCase.start_tour` (the URL path first, then the tour name and options). If the environment variable `HAMS_TOUR_TOUR_DEBUG` is set and the first argument is a string URL path, its `debug=` query parameter is set to that value (replacing an existing one, else appended with `?` or `&`). It then calls core's `start_tour`, which drives the tour through `browser_js`. If anything is raised it logs the error, sets the class flag `_hams_tour_failed = True` (which keeps the V8 hang logs for inspection) and re-raises the same exception (`from None`). Returns nothing. Errors: whatever core raises (`AssertionError` for a failed tour, `ChromeBrowserException`).

### browser_js Wrapper `[@ANCHOR: zero_sudo:hams_http_case_browser_js]`

**Signature.** `browser_js(self, *args, expect_empty=False, **kwargs)`: the arguments of core `HttpCase.browser_js` plus `expect_empty`. Before running, a run of `/web/tests` or `/web/tests/legacy` sets `content_security_policy.allow_developer_pages` to `True` (the test is asking for that page; the test transaction rolls it back). It attaches an empty-run detector to the browser console logger, prepends the navigator overrides and a set of suppressions for known harmless page errors to the `ready` script, delegates to core, and in a `finally` detaches the detector. If the detector saw "Passed 0 tests" and `expect_empty` is false, the test fails (see Empty-Run Guard). Returns nothing; raises what core raises. A tour reaches this method through `start_tour`; the `ChromeBrowser` it builds is pooled per class unless the pool does not apply.

### Empty-Run Guard `[@ANCHOR: zero_sudo:hoot_empty_run_detector_emit]`

A logging handler the `browser_js` wrapper attaches to the browser-console logger for the duration of each run, so that a hoot suite which executed NO tests fails instead of passing silently. Hoot reports an empty run as `Passed 0 tests` followed by `Test suite succeeded` -- and `Test suite succeeded` is exactly the signal `browser_js` waits for, so before this guard a wrapper over an empty suite passed every time while testing nothing. A deliberately empty run must say so by passing `expect_empty=True`; see the `linter-compliance` skill's own hoot rules for why deleting the wrapper is usually the better answer. Note this guard lives on `HamsHttpCase`'s override and therefore does NOT cover the separate module-level patch immediately below, which `RealTransactionCase` reaches instead.

### Global HttpCase.browser_js Patch `[@ANCHOR: zero_sudo:patched_browser_js]`

`_patched_browser_js(self, *args, **kwargs)` is a SEPARATE, module-level monkeypatch of core Odoo's `HttpCase.browser_js` (not `HamsHttpCase`'s own override), reached by `RealTransactionCase`, which extends `HttpCase` directly and has no override to shadow it. It calls the original; on any exception, when `HAMS_PAUSE_ON_FAIL=1` (set by the `--pause-on-fail` option of `test.py`) it logs that the tour failed and sleeps in a loop of 60 s forever so a developer can attach DevTools to the fixed port 9222 of the frozen headless Chrome; with the variable unset it re-raises the exception unchanged. It adds no empty-run guard: that lives only in the `HamsHttpCase` wrapper. Never use the pause mode in an unattended run; it never returns.

## Chrome/DevTools Protocol Monkeypatches
A cluster of module-level patches make headless-Chrome tour testing reliable in this sandboxed environment, each addressing one real, previously-observed failure mode:
### Fetch Interception `[@ANCHOR: zero_sudo:patched_handle_request_paused]`

`_patched_handle_request_paused(self, *args, **kwargs)` replaces `ChromeBrowser._handle_request_paused`, which receives the DevTools `Fetch.requestPaused` event for every request the test browser makes. It decides what each request may do: a URL at the real test server's origin (`http://` or `https://` on `HOST:<http_port>`, matched at a real authority boundary by `_url_is_at_origin`), at an `extra_allowed_fetch_hosts` host (any port), or a `data:` or `about:` URL, continues (`Fetch.continueRequest`); any other URL is answered by `test_case.fetch_proxy(url)` through `Fetch.fulfillRequest` when the test set one, else fails with `Fetch.failRequest` (`errorReason: Failed`). So a browser test can never reach an external host by accident. The decision is sent over the browser's websocket; a send failure is logged and swallowed. Returns nothing. Tests: `test_extra_allowed_fetch_hosts_default_is_unchanged`, `_opt_in`, `test_real_server_allowlist_is_port_specific`.

### Process Group Isolation `[@ANCHOR: zero_sudo:patched_preexec]`

`_patched_preexec(*args, **kwargs)` replaces core's `_preexec`, the function Odoo runs in the forked child just before it executes Chrome (only when core has one). It calls the original, then `os.setsid()` so the browser starts a new session and process group of its own; killing the test runner then does not leave orphaned Chrome processes in the runner's group, and the harness can signal the browser's group without touching its own (`_kill_browser_tree` checks this). An error from `setsid` (already a session leader) is logged at info level and ignored. It takes the same arguments as the original and returns nothing. It runs between `fork` and `exec`, so it must stay tiny and must not take locks.

### Chrome Spawn `[@ANCHOR: zero_sudo:patched_spawn_chrome]`

`_patched_spawn_chrome(self, *args, **kwargs)` replaces `ChromeBrowser._spawn_chrome`. First it calls `_kill_all_headless_chrome_for_this_uid()`, which kills the headless Chrome processes of this test run (descendants and leftovers under this process's temp directory, never another run's), preventing port and profile collisions. Then on the command list (first argument or `cmd`) it inserts, when absent, `--ignore-certificate-errors`, `--disable-dev-shm-usage`, `--disable-site-isolation-trials`, `--js-flags=--max-old-space-size=256`, `--disable-features=TranslateUI,BlinkGenPropertyTrees`, `--disable-extensions` and `--disable-background-networking`, and prefixes the command with `setsid -w unshare -c -m -p -f --mount-proc --kill-child=SIGTERM`, so Chrome and every descendant (GPU, renderers) live in their own PID namespace and die when the `unshare` parent is terminated. Then it calls the original and returns its result. It starts a real process.

### Werkzeug Request Thread `[@ANCHOR: zero_sudo:patched_process_request_thread]`

`_patched_process_request_thread(self, request, client_address, *args, **kwargs)` replaces `werkzeug.serving.ThreadedWSGIServer.process_request_thread`, the body of each thread that serves one HTTP request in the test server. It adds the current thread to the module set `_active_werkzeug_threads` before calling the original, and in a `finally` removes it from that set and from `_abandoned_werkzeug_threads`. With `HAMS_TRACE_PG_THREADS=1` it logs each add and discard at warning level. The set is what `wait_for_werkzeug_threads` joins before a `RealTransactionCase` drops its cursor, so a daemon's in-flight RPC cannot outlive the test. It returns whatever the original returns and re-raises its exceptions after the bookkeeping. The set is process-wide and is not locked: set `add` and `discard` are atomic enough in CPython.

### Screenshot Saving `[@ANCHOR: zero_sudo:patched_save_test_file]`

`_patched_save_test_file(test_name, content, prefix, extension="png", logger=..., document_type="Screenshot", date_format=..., loglevel=logging.RUNBOT, directory="", ...)` replaces core's `save_test_file`, which writes a tour's failure screenshot or log. It does nothing unless `SAVE_LOGS=1`. Then it writes `<prefix>_<timestamp>_PID<pid>_<test_name>.<extension>` under `directory` (a relative directory is forced under `/opt/hams/test`, or `HAMS_REAL_LOG_DIRECTORY` when not in an isolated namespace, and a path that resolves outside that base raises `ValueError("Path traversal detected")`) and copies it to the host directory, chown-ing it to `SUDO_USER` (default `odoo`) when that user exists. `content` is `str` or `bytes`. Filesystem and chown errors are logged as warnings, never raised; returns nothing.

### Opener Init `[@ANCHOR: zero_sudo:patched_opener_init]`

`_patched_opener_init(self, *args, **kwargs)` replaces `odoo.tests.common.Opener.__init__`, the HTTP session that `url_open` uses. It calls the original constructor and then sets `self.verify = False`, because the test proxy and Chrome use a self-signed certificate (the same reason `--ignore-certificate-errors` is passed to Chrome). It takes the original's arguments and returns nothing. The effect is limited to test sessions in a test process, never production code, and `HamsHttpCase.setUp` also disables urllib3's insecure-request warning.

### Chrome Init `[@ANCHOR: zero_sudo:patched_chrome_init]`

`_patched_chrome_init(self, *args, **kwargs)` replaces `ChromeBrowser.__init__`. With `HAMS_PAUSE_ON_FAIL=1` it sets the instance attribute `remote_debugging_port = 9222` (not the class attribute, so later browsers built without the variable keep Odoo's ephemeral port). It then calls the original constructor up to 3 times: on any exception it logs a warning, calls `_tear_down_partly_built_browser(self)` (see Retry Teardown), sleeps 2 s and retries; after the third failure it re-raises. Once Chrome is up, outside the retry, it installs the navigator overrides and, when `HAMS_JS_COVERAGE_DIR` is set, starts coverage; a failure there must not trigger a second constructor pass on a built object. Returns nothing; may raise what the original raises (`unittest.SkipTest` when Chrome cannot start).

### Chrome Stop `[@ANCHOR: zero_sudo:patched_chrome_stop]`

`_patched_chrome_stop(self, *args, **kwargs)` replaces `ChromeBrowser.stop`. For a browser that owns its Chrome (the process is `self.chrome`, still running, pid above 1; a pooled browser's Chrome belongs to the pool and is skipped) it kills every descendant process with `psutil`, skipping any pid in `_protected_pids()` (this process, its ancestors, init) and ignoring processes that already exited. Then it calls the original `stop` and joins the websocket receiver thread (`_receiver`) for up to 2 s if it is still alive. Returns nothing. It signals only the browser's own descendants, never a parent or a group, and a protected pid found under the browser is logged and left.

### Wait-Ready `[@ANCHOR: zero_sudo:patched_wait_ready]`

`_patched_wait_ready(self, ready_code=None, timeout=60, *args, **kwargs) -> bool` replaces `ChromeBrowser._wait_ready`, a line-for-line mirror of Odoo 19's with two changes. Core re-issued `Runtime.evaluate` in a tight loop (about 110,000 calls in 60 s) and the last call got a near-zero timeout that escaped as a bare `TimeoutError`. Here the loop sleeps `_WAIT_READY_POLL_INTERVAL` (0.1 s, or the remaining time if shorter) between polls and a `TimeoutError` from one evaluate counts as "not ready yet". `timeout` is multiplied by `throttling_factor`; `ready_code` defaults to `document.readyState === 'complete'` (logged cut to 150 characters). Returns `True` when the expression evaluates to boolean true; otherwise, at the deadline, raises the browser's pending exception if any, else saves a `sc_failed_ready_` screenshot and returns `False`. A cancelled request is turned into that pending exception.

### Chrome Start `[@ANCHOR: zero_sudo:patched_chrome_start]`

`_patched_chrome_start(self, *args, **kwargs)` is a context manager replacing `ChromeBrowser._chrome_start`, which yields `(process, devtools_port)`. When `_pool_applies` is true it delegates to `_pooled_chrome_start` and the class's shared Chrome is leased. Otherwise (the pool's own start-up excepted) it first closes the pool, since the spawn reaps every browser of the run, then runs core's start-up and remembers the Chrome's children. On exit, even after an error, it kills the browser tree (`_kill_browser_tree`), terminates and kills the process, kills the remembered children not in `_protected_pids()`, and finally kills any `chrome` or `cat` descendant of the test process and reaps zombies. `psutil.NoSuchProcess` during the lifecycle is ignored, and each cleanup step logs its own error without raising, so a Chrome that GC'd itself cannot fail a test.

### Retry Teardown `[@ANCHOR: zero_sudo:tear_down_partly_built_browser]`

Core's `ChromeBrowser.__init__` has no error handling of its own, so a failed attempt (retried by the patched Chrome Init above) used to leave its partially-registered resources (profile directory, process, websocket, receiver thread) orphaned when the retry overwrote the same object's `cleanup` stack -- worse, the orphaned receiver thread's own teardown then ran against the SUCCEEDING attempt's healthy socket, silently breaking every CDP call for the rest of that browser's life. This closes out whatever the failed attempt actually registered before the retry starts from a clean object.

### wait_for_werkzeug_threads `[@ANCHOR: zero_sudo:wait_for_werkzeug_threads]`

**Signature.** `wait_for_werkzeug_threads(timeout=5.0)`; `timeout` is the total number of seconds to spend joining all tracked request threads. **Returns** nothing. For each thread in `_active_werkzeug_threads` (the current thread and already-abandoned ones are skipped) it joins within the remaining time. A thread still alive is sent `SystemExit` through `PyThreadState_SetAsyncExc`; CPython only records it, so the function waits 0.5 s more and, if the thread is still alive (blocked in a C call such as a socket read), logs an error and adds it to `_abandoned_werkzeug_threads`, so later tests do not pay the timeout again. A failed injection is logged. No exception is raised. `RealTransactionCase._real_teardown` calls it before dropping its raw cursor so a daemon's in-flight RPC request cannot outlive the test and cause a `SerializationFailure` on the way out. Side effects: it can end request threads mid-work, which is acceptable only in a test process.

### Allowing Extra Fetch Hosts `[@ANCHOR: zero_sudo:extra_allowed_fetch_hosts]`

The Fetch interception above lets a browser test reach only `HOST` (127.0.0.1) for real; every other host fails the request, so a test cannot make an unbounded real external request by accident. `HttpCase.extra_allowed_fetch_hosts` (default: an empty tuple) is the opt-in for a test that needs a real round trip to one other, hermetic host it controls, such as a fake relay served by an `HTTPServer` the same test starts and stops. The test sets it to a tuple of literal host names and the interception then lets those hosts through exactly as it does `HOST`. With nothing set, behaviour is unchanged.

### Navigator Overrides `[@ANCHOR: zero_sudo:install_navigator_overrides]`

Registers, through the DevTools protocol, a script that runs at the start of every document the test browser loads. It makes `navigator.onLine` read true and gives `navigator.virtualKeyboard` a harmless stub, both defined on `Navigator.prototype` and never on the `navigator` instance, because hoot's `createMock(navigator, ...)` breaks when the instance has own keys. Without it the sandbox's headless Chromium reports itself offline, which sent every tour's writes into the offline queue, and its VirtualKeyboard API crashed web's `dvu.js` at module load. A failure to register is logged and tolerated: the same script still runs from the `ready` step of `browser_js`, only later.

### Reaping Leaked Headless Chrome `[@ANCHOR: zero_sudo:kill_all_headless_chrome]` / Which Browsers Are This Run's `[@ANCHOR: zero_sudo:headless_chrome_of_this_run]`

Kills every headless `chrome`, `chromium` or `chromium-browser` process of the current uid that belongs to THIS test process: a descendant of it, or an orphan whose `--user-data-dir` is a directory under this process's `tempfile.gettempdir()` that exists in its own view of the filesystem (what a tour whose parent died leaves). It runs before a new Chrome is spawned and at the end of every test, so memory held by a tour that did not tear down completely is reclaimed at once. It used to kill every headless browser of the uid on the machine, which was safe only while `test.py` allowed one run at a time; with test slots (ADR 0116) several runs share the machine, a run in its own PID namespace is visible in the host's process table, and the old reap killed other runs' browsers mid-tour (exit code 9, `CancelledError` in `Page.navigate`, a stalled tour step). A non-headless browser, another user's process, any process that is not one of those three browsers, and another run's browser are left alone. The function name keeps `_for_this_uid` for history; `_headless_chrome_of_this_run` returns the processes (psutil objects) without killing them.

### Hypothesis Profile for Property Tests `[@ANCHOR: zero_sudo:hypothesis_profile]`

Every property test that runs in an Odoo test process (a `@given` test in `ham_com`, `ham_base`, `ham_dns`, `ham_propagation`, `ham_repeater_dir`, `ics_forms`, `edge_routing`) runs under one registered Hypothesis settings profile, `hams_odoo_tests`, so a loaded machine cannot fail it. Found 2026-11-08: the whole-system gate ran at load 100 and `TestForgivingCallsignDomainProperties.test_domain_with_no_callsign_field_is_unchanged` failed with `FailedHealthCheck: Input generation is slow: Hypothesis only generated 1 valid inputs after 38.03 seconds`, a timing check and not a correctness one.
  - **Call interface:** `register_hams_profile()` in `zero_sudo/tests/hypothesis_profile.py`; no arguments, returns `None`, raises nothing. `zero_sudo.tests.common` calls it when it is imported (every `HamsTransactionCase`, `HamsHttpCase` and sibling gets it), and a property test that does not use the harness (edge_routing's) calls it itself before it builds its settings.
  - **The profile:**

    | Setting | Value | Why |
    |---|---|---|
    | `deadline` | `None` | A slow example is not a wrong example; at high load a 1 ms example took hundreds of ms. |
    | `derandomize` | `True` | The same examples on every run, so a loaded machine and an idle one run the same checks and a failure repeats. |
    | `max_examples` | `200` | The default for a test that names none; a test's own `settings(max_examples=N)` still wins. |
    | `suppress_health_check` | `[too_slow]` only | `too_slow` is the load-induced one. Every other check stays on: `data_too_large`, `filter_too_much`, `function_scoped_fixture`, `large_base_example` and the rest still fail a badly designed strategy. |

  - **Special concerns:** a `settings(...)` object a test file builds at import (`_SETTINGS = settings(max_examples=300, deadline=None)`) inherits every field it does not name from the loaded profile, so those files need no change. The profile is process-wide, which is the intent: the property tests are the only Hypothesis users in the Odoo process. No effect on the daemons' and tools' pytest runs, which have their own configuration (`hams_shared/tools/conftest.py`).
  - **See also:** `[@ANCHOR: zero_sudo:test_hypothesis_profile_is_load_proof_and_keeps_other_checks]`.

## One Chrome per Test Class
A browser test used to start a Chrome (about 0.7 s) and stop it (0.5 to 1 s) of its own, most of a 2.5 s tour. The harness now keeps one headless Chrome alive for the tests of one class and gives every browser test a fresh **browser context** of it, an incognito-style profile created with the DevTools command `Target.createBrowserContext`. Cookies, local and session storage, IndexedDB, the Cache API, service workers and the HTTP cache of one test are invisible to the next, and `Target.disposeBrowserContext` at the end of the test destroys them together with the test's renderers. What is shared is the browser process, the GPU process and the profile directory, not any page state. Set `HAMS_CHROME_POOL=0` to get a Chrome per browser again. The code is `zero_sudo/tests/chrome_pool.py` (the pool) and the hooks in `zero_sudo/tests/common.py`; the tests are `zero_sudo/tests/test_chrome_pool.py`.

### ChromePool `[@ANCHOR: zero_sudo:chrome_pool]`

`class ChromePool` in `zero_sudo/tests/chrome_pool.py` holds at most one live Chrome, leased to one browser test at a time (Odoo runs the tests of a process one after the other, and an `RLock` guards its state anyway). State: `_start` (the harness's start callable), `_max_uses`, `_lock`, `_chrome` (the live `PooledChrome` or `None`) and the counter `starts` (Chromes started so far, used by tests and logs). Methods: `acquire`, `new_page`, `end_page`, `release`, `close`, `owned_pids`, `owns_profile`, each below. The module singleton is `_chrome_pool` in `common.py`, closed by `HamsHttpCase.tearDownClass` and by `atexit`. Set `HAMS_CHROME_POOL=0` to disable pooling.

### ChromePool.__init__ `[@ANCHOR: zero_sudo:chrome_pool_init]`

`ChromePool(start, max_uses=40)`. `start(touch_enabled)` is a callable the harness supplies that starts a headless Chrome and returns `(proc, devtools_port, user_data_dir, stop)`: the `Popen` object, the DevTools port, the profile directory and a function that stops that Chrome. `max_uses` is the number of leases (tests) one Chrome serves before it is replaced (`MAX_USES` = 40), which bounds what a long class accumulates. The constructor creates the lock, sets `_chrome` to `None` and `starts` to 0, and starts nothing: the first `acquire` does. The pool never imports the harness, so it can be tested with a stub `start`.

### ChromePool.chrome `[@ANCHOR: zero_sudo:chrome_pool_chrome]`

`ChromePool.chrome` is a read-only property returning the live `PooledChrome`, or `None` when no Chrome is running. It takes no lock, so the value is a snapshot: a caller must not assume it is still the live one after another call into the pool. It is used by the tests of the pool and by diagnostics to see which Chrome is currently held; the harness's own code reaches the Chrome through the object `acquire` returns, and through `owned_pids` and `owns_profile`.

### PooledChrome `[@ANCHOR: zero_sudo:pooled_chrome]`

`class PooledChrome` is one live Chrome of the pool. Fields: `proc` (the `subprocess.Popen` of the Chrome or of its namespace wrapper), `devtools_port` (the remote debugging port on `HOST`), `user_data_dir` (the profile directory, removed by `stop`), `owner` (the test class it was started for, compared by identity), `touch_enabled` (bool; touch events are a launch switch, not a per-page setting), `uses` (leases so far) and the private `_stop` (the stop callable, set to `None` once used). It is created only by `ChromePool.acquire`; its methods `alive`, `pids`, `log_size`, `log_since`, `command` and `stop` follow. Not thread-safe by itself: the pool serialises access.

### PooledChrome.__init__ `[@ANCHOR: zero_sudo:pooled_chrome_init]`

`PooledChrome(proc, devtools_port, user_data_dir, stop, owner, touch_enabled)` only records its arguments and sets `uses = 0`; it starts, checks and connects to nothing. `proc` is the `Popen` object, `devtools_port` an int, `user_data_dir` the profile directory path (the pool deletes it on `stop`), `stop` the function that terminates the Chrome and its process tree (the harness's `_patched_chrome_start` exit), `owner` the test class the Chrome was started for, and `touch_enabled` whether it was launched with touch events. Ownership: the pool owns the object and the files and process it names.

### ChromePoolError `[@ANCHOR: zero_sudo:chrome_pool_error]`

`class ChromePoolError(Exception)`: raised by `PooledChrome.command` when a DevTools command to the pooled Chrome returns an error reply (the message is `<method>: <Chrome's message>`) or no answer within its timeout (`<method>: no answer within <t> s`). It means "this Chrome is not usable": `ChromePool.new_page` disposes its half-made context and re-raises it, `end_page` turns it into a `False` result, and callers treat the Chrome as dirty so that it is replaced. It carries no extra fields and is never raised for a test failure.

### ChromePool.acquire `[@ANCHOR: zero_sudo:chrome_pool_acquire]`

**Signature.** `acquire(owner, touch_enabled) -> PooledChrome`. `owner` is the test case's class, `touch_enabled` a bool. **Returns** the live Chrome for a test of class `owner`, starting one with `start(touch_enabled)` when there is none, and increments its `uses`. A live Chrome is stopped and replaced first when it was started for another class, needs other launch switches, has served `max_uses` tests, is not running, or its DevTools endpoint (`/json/version`) does not answer within 3 s; the reason is logged. **Errors.** Whatever `start` raises propagates (a Chrome that cannot be started raises `unittest.SkipTest`, as core's does) and leaves the pool empty. **Locking.** Holds the pool's reentrant lock for the whole call, including the start and the liveness probe.

### ChromePool.new_page `[@ANCHOR: zero_sudo:chrome_pool_new_page]`

**Signature.** `new_page(chrome) -> (context_id, page_websocket_url)`. `chrome` is the leased `PooledChrome`. It creates a browser context with the DevTools command `Target.createBrowserContext` (`disposeOnDetach` false), then a blank `about:blank` page in it with `Target.createTarget`, and returns the context id and the page's `ws://HOST:<port>/devtools/page/<targetId>` URL for the test's own connection. The context is an incognito-style profile: cookies, local and session storage, IndexedDB, the Cache API, service workers and the HTTP cache are invisible to other contexts. **Errors.** `ChromePoolError` from either command; when the page cannot be created the context is disposed before the error is re-raised. It talks to the browser, not a page, over a short-lived websocket.

### ChromePool._dispose_context `[@ANCHOR: zero_sudo:chrome_pool_dispose_context]`

`ChromePool._dispose_context(chrome, context_id) -> bool` (a static method) sends `Target.disposeBrowserContext` for `context_id`, which destroys the test's pages, storage, cookies and renderers together. It returns `True` on success and `False` on any exception (a crashed Chrome raises whatever its socket does, so it catches broadly and logs a warning). It never raises, because teardown must continue; the caller turns a `False` into "mark this Chrome dirty". `new_page` uses it to undo a half-made context and `end_page` to finish a test.

### ChromePool.end_page `[@ANCHOR: zero_sudo:chrome_pool_end_page]`

`end_page(chrome, context_id) -> bool` destroys a test's browser context (its pages, storage, cookies, service workers) at the end of the test by calling `_dispose_context`. It returns `False` when disposal failed, which makes the Chrome untrustworthy (crashed or hung), and the harness then releases it as unclean so it is replaced and only the test that used it fails. `True` means the isolation boundary was destroyed cleanly. No lock is taken; the websocket is short-lived. It never raises.

### ChromePool.release `[@ANCHOR: zero_sudo:chrome_pool_release]`

`release(chrome, clean)` ends a lease; `clean` is a bool. A clean test leaves the Chrome in the pool for the next test of the class. An unclean one (an exception left the browser's start-up or body, `had_failure`, a failed result, or a context that could not be disposed) stops it with `close()`, so a crashed or hung Chrome fails only the test that used it. Releasing a Chrome the pool no longer holds (already replaced) changes nothing, so it can never stop its successor. Holds the lock; returns nothing; does not raise.

### ChromePool.close `[@ANCHOR: zero_sudo:chrome_pool_close]`

`close()` stops the live Chrome, if any: under the lock it detaches `_chrome`, then calls the Chrome's `stop()` outside the lock. Called by `HamsHttpCase.tearDownClass`, at interpreter exit (`atexit`), before a browser that is not pooled spawns its own Chrome (its spawn reaps every browser of the run), and by `acquire` and `release` when they replace a Chrome. Safe to call with nothing running and safe to call twice. It signals the Chrome's own process tree only and returns nothing.

### ChromePool.owned_pids `[@ANCHOR: zero_sudo:chrome_pool_owned_pids]`

`owned_pids() -> set[int]` returns the pids of the live Chrome and everything below it, or an empty set. `_headless_chrome_of_this_run`, the end-of-test reaper, leaves these alone; otherwise it would kill the class's Chrome after every test. The pids are read from `psutil` at call time, so they include renderer and GPU processes started since the lease began, whichever PID namespace they run in. The pool lock is held only to read the reference.

### ChromePool.owns_profile `[@ANCHOR: zero_sudo:chrome_pool_owns_profile]`

`owns_profile(profile_dir) -> bool` is `True` when `profile_dir` is the profile directory of the live Chrome, comparing real paths (`os.path.realpath`). The reaper `_headless_chrome_of_this_run` calls it for each `--user-data-dir` on a candidate process's command line, so it does not kill a Chrome that runs from the pool's profile even when its pid is not yet in `owned_pids` (for example a process in another PID namespace). `False` when no Chrome is live. Pure apart from the lock.

### PooledChrome.alive `[@ANCHOR: zero_sudo:pooled_chrome_alive]`

`alive() -> bool` is true while the process is running (`proc.poll()` is `None`) and its DevTools endpoint `http://HOST:<port>/json/version` answers a successful status within 3 s. It returns `False` for an exited process and for any `requests` exception (connection refused, timeout). It is the health check `acquire` runs on a Chrome before reusing it, so a hung Chrome is replaced. It blocks for up to 3 s and has no side effects.

### PooledChrome.pids `[@ANCHOR: zero_sudo:pooled_chrome_pids]`

`pids() -> set[int]` returns the pid of the Chrome's `Popen` process plus the pids of all its descendants (renderers, the GPU process) found with `psutil.Process(...).children(recursive=True)`, which works whichever PID namespace they run in. A `psutil` error while listing is logged at debug level and the pids found so far (at least the process itself) are returned. It is the source of `ChromePool.owned_pids`. Read only.

### PooledChrome.log_size `[@ANCHOR: zero_sudo:pooled_chrome_log_size]`

`log_size() -> int` is the current size in bytes of `chrome_debug.log` in the Chrome's profile directory, or 0 when the file does not exist or cannot be read (`OSError`). `_pooled_chrome_start` records it when a test's lease begins; `log_since` then gives only the bytes written afterwards, so one test's log is not mixed with its predecessors'. Read only, no arguments.

### PooledChrome.log_since `[@ANCHOR: zero_sudo:pooled_chrome_log_since]`

`log_since(offset) -> bytes` returns the bytes the Chrome has written to its `chrome_debug.log` since `offset`, a value from `log_size()`. It opens the file, seeks and reads to the end; a missing or unreadable file gives `b""`. This is the part of the log that belongs to one test; the patched `read_log` returns it to core, which saves it on failure. Read only.

### PooledChrome.command `[@ANCHOR: zero_sudo:pooled_chrome_command]`

**Signature.** `command(method, params=None, timeout=10.0) -> dict`. Runs one browser-level DevTools command (not a page command), for example `Target.createBrowserContext`. It reads `webSocketDebuggerUrl` from `/json/version`, opens a short-lived websocket, sends `{"id": 1, "method": method, "params": params or {}}` and reads replies until the one with id 1, then always closes the socket. **Returns** the reply's `result` dict (`{}` if absent). **Errors.** `ChromePoolError` on an error reply or when no answer arrives within `timeout` seconds; `requests` and websocket exceptions from a dead Chrome propagate. It blocks, with no locks.

### PooledChrome.stop `[@ANCHOR: zero_sudo:pooled_chrome_stop]`

`stop()` runs the harness's stop function (the same termination and process-tree kill every test Chrome gets) and removes the profile directory with `shutil.rmtree(..., ignore_errors=True)`. The stop function is taken once and cleared, so calling `stop()` twice is safe, and an exception from it is logged as a warning and does not prevent the directory removal. Returns nothing. It signals the Chrome's own tree only, never a parent or a group. Called by `ChromePool.close`.

### _start_pooled_chrome `[@ANCHOR: zero_sudo:start_pooled_chrome]`

`_start_pooled_chrome(touch_enabled) -> (proc, devtools_port, user_data_dir, stop)` is the pool's `start` callable (`_chrome_pool = ChromePool(_start_pooled_chrome)`). It builds a stand-in `ChromeBrowser` (created with `__new__`, carrying only what start-up reads: a logger, `_hams_unpooled = True` so the pooled path is not chosen again, a log level, a cleanup stack and a fresh temp profile directory `*_chrome_odoo`), enters `_patched_chrome_start` on it with `headless=True`, `debug=False` and the given `touch_enabled`, and returns the process, port, directory and a function that exits that context manager (stopping Chrome). If start-up fails the profile directory is removed again and the error propagates. It starts a real process with the usual protections (own process group, PID namespace, reaper).

### _profiles_of `[@ANCHOR: zero_sudo:profiles_of]`

`_profiles_of(process) -> list[str]` returns the values of every `--user-data-dir=...` argument in the command line of a `psutil` process (`process.info["cmdline"]`, which the caller requested through `process_iter`), in order; an empty list when there is none or no command line. `_headless_chrome_of_this_run` compares them with the pool's profile through `owns_profile`, so the pooled Chrome is not reaped as a leftover. Pure.

### _pool_applies `[@ANCHOR: zero_sudo:pool_applies]`

`_pool_applies(browser, launch_kwargs) -> bool` says whether a `ChromeBrowser` may share the class's Chrome. True only when all hold: it is not the pool's own start-up stand-in (`_hams_unpooled`); `HAMS_CHROME_POOL` is not `0`; `HAMS_PAUSE_ON_FAIL` is not `1` (it needs the fixed DevTools port 9222); `HAMS_OFFLINE_NS_CTL` is unset (an offline-isolation run uses a Chrome in its own network namespace); `launch_kwargs.get("headless", True)` is true; and `launch_kwargs.get("debug", False)` is `False` (a visible or debugging browser). A false result sends the browser down core's path with a Chrome of its own. Reads the environment; no side effects.

### _pooled_chrome_start `[@ANCHOR: zero_sudo:pooled_chrome_start]`

`_pooled_chrome_start(self, touch_enabled)` is a context manager that replaces core's `_chrome_start` for a pooled browser. It leases the class's Chrome with `_chrome_pool.acquire(type(self.test_case), touch_enabled)`, stores it as `self._hams_chrome`, records the log offset (`_hams_log_offset`), sets `_hams_page_ok = True`, and yields `(chrome.proc, chrome.devtools_port)`, which is what core expects. On exit, if an exception left the block it marks `_hams_page_ok = False`; then it releases the Chrome with `clean=_pooled_test_was_clean(self)` instead of terminating it. Errors from `acquire` (`SkipTest`) propagate before anything is leased.

### _pooled_test_was_clean `[@ANCHOR: zero_sudo:pooled_test_was_clean]`

`_pooled_test_was_clean(browser) -> bool` is `True` when a pooled browser's test ended without any sign of trouble, so its Chrome can be reused. It is `False` when `_hams_page_ok` is false (an exception left the start-up or body, or the context could not be created or disposed), when `browser.had_failure` is true (a JavaScript failure was reported), or when the browser's `_result` future finished with an exception that was not a cancellation. Missing attributes count as clean. Anything else makes `release` stop and replace the Chrome. Read only.

### Page Websocket `[@ANCHOR: zero_sudo:patched_open_websocket]`

`_patched_open_websocket(self, *args, **kwargs)` is a context manager replacing `ChromeBrowser._open_websocket`, which yields the DevTools websocket of the test's page. For a browser with no `_hams_chrome` it delegates to core unchanged. For a pooled browser it calls `_chrome_pool.new_page(chrome)`, connects to that new page's websocket (`enable_multithread=True`, no Origin header, timeout 0.01 s), skips the test with `unittest.SkipTest` if the handshake status is not 101, yields the socket, and at the end closes it and destroys the context with `end_page`. Any failure while opening, or a failed disposal, sets `_hams_page_ok = False` (the Chrome will be replaced) and a context made before the failure is disposed.

### Browser Close `[@ANCHOR: zero_sudo:patched_websocket_request]`

`_patched_websocket_request(self, method, *args, **kwargs)` replaces `ChromeBrowser._websocket_request`, the call that sends one DevTools command. It returns `None` without sending when `method == "Browser.close"` and the browser is pooled (`_hams_chrome` is set): `Browser.close` is core's last shutdown step and would end the class's shared Chrome. Every other method, and every command of an unpooled browser, goes to the original and its result is returned. No state; no errors of its own.

### Chrome Log `[@ANCHOR: zero_sudo:patched_read_log]`

`_patched_read_log(self, *args, **kwargs)` replaces `ChromeBrowser.read_log`, which core uses to save Chrome's log when a test fails. For an unpooled browser it calls the original. For a pooled browser it returns `chrome.log_since(self._hams_log_offset)` (bytes): only what the shared Chrome wrote since this test's lease began, so a failure report does not contain the log of the whole class. Read only.

## Harness Helpers and Base-Class Set-Up (Whole Interface)

- **Origin Match** `[@ANCHOR: zero_sudo:url_is_at_origin]`: `_url_is_at_origin(url, origin, allow_any_port=False)` returns True when `url` starts with `origin` and the origin ends at a real authority boundary: the next character must start the path, query or fragment, or the URL must end. A bare prefix test is wrong because `http://127.0.0.1:8069` is a prefix of `http://127.0.0.1:80699/x` (another port) and of `http://127.0.0.1:8069@example.com/` (whose host is `example.com`). `allow_any_port=True` also accepts `:` so a bare host matches on any port. Pure, no side effects; used by the fetch interception that lets a browser test reach only the test server. **Tests:** `test_an_origin_matches_only_at_a_real_authority_boundary`.
- **Protected Pids** `[@ANCHOR: zero_sudo:protected_pids]`: `_protected_pids()` returns the set of pids the browser clean-up must never signal: 0, 1, this process and every ancestor of it (a failed ancestor lookup is logged and the rest is still returned). No arguments, no side effects. **Tests:** `test_the_protected_pids_are_this_process_init_and_every_ancestor`.
- **Browser Tree Kill** `[@ANCHOR: zero_sudo:kill_browser_tree]`: `_kill_browser_tree(proc, where)` SIGKILLs a test browser's descendants and then its process group, but only when that group is the browser's own. `proc` is a `subprocess.Popen`; `where` is a label for the log. It does nothing for a missing pid, a pid at or below 1 or an already-exited browser (its pid number may belong to someone else by now); it skips any descendant in `_protected_pids()`; and it never signals a group that is ours, at or below 1 or protected. It logs pid, pgid and our own group first. Returns nothing and swallows `OSError` and `NoSuchProcess`. **Tests:** `test_a_live_browser_tree_is_killed_but_never_this_process_or_a_foreign_group`, `test_the_browser_clean_up_leaves_our_own_process_group_and_exited_browsers_alone`.
- **JS Coverage Start** `[@ANCHOR: zero_sudo:js_coverage_start]`: `_js_coverage_start(browser)` is a no-op unless the environment variable `HAMS_JS_COVERAGE_DIR` is set. Then it enables the Profiler and Debugger domains and precise coverage over the browser's own DevTools session, marks the browser, and registers `_js_coverage_write` on `browser.cleanup` so it runs when the browser shuts down. Best effort: a timeout, OS error, missing attribute or DevTools error is logged as a warning and never fails the test. **Tests:** `test_js_coverage_starts_only_when_asked_and_never_fails_the_test`, `test_js_coverage_is_written_when_the_browser_shuts_down`.
- **JS Coverage Write** `[@ANCHOR: zero_sudo:js_coverage_write]`: `_js_coverage_write(browser)` runs once per browser (it clears its mark first) and only when coverage was started. It takes the coverage, keeps the scripts served from `/web/assets/`, fetches each bundle's exact source from the debugger (a script the page already left has no source and is skipped), and writes one JSON record `{test, scripts, bundles}` named after the test id and a nanosecond timestamp into `HAMS_JS_COVERAGE_DIR`. Nothing is written when no bundle was loaded. Best effort, as above. **Tests:** `test_js_coverage_is_written_when_the_browser_shuts_down`.
- **HamsTransactionCase Class Set-Up** `[@ANCHOR: zero_sudo:hams_transaction_case_setup_class]`: `setUpClass` gives the class its own `_active_daemons` list (the base-class list would be shared by every subclass), generates a Fernet test key in `_hams_test_crypto_key`, patches the two `read_secret` readers with it through `startClassPatcher` (so a failed set-up still undoes the patches), sets `HAMS_CRYPTO_KEY` if unset, runs Odoo's set-up, writes `web.base.url` (`https://hams.com`) and `web.base.url.freeze` (`1`) with a SQL upsert, and clears the stable cache and the `zero_sudo` utils cache. It touches the database and the process environment. **Tests:** `test_set_up_class_gives_the_class_its_own_daemon_list_a_valid_key_and_the_frozen_base_url`.
- **Daemon Start** `[@ANCHOR: zero_sudo:hams_transaction_case_start_daemon]`: `start_daemon(script_path, args=None, env_vars=None, health_url=None, timeout=600)` starts a daemon script through `zero_sudo.daemon.utils`, records it in the class's `_active_daemons` (torn down by `tearDownClass`) and returns the `Popen`. With `health_url` it polls (every 0.5 s, up to `timeout` seconds) until the URL answers 200 or 204; it raises `RuntimeError` if the daemon exits first and `TimeoutError` if it never answers. It starts a real process and blocks while waiting. **Tests:** `test_01_daemon_lifecycle` in `tests/test_integration.py`.
- **HamsHttpCase Class Set-Up** `[@ANCHOR: zero_sudo:hams_http_case_setup_class]`: `setUpClass` does what the transaction-case set-up does (key, patches, base-URL rows, caches), with Odoo's set-up run while threads start as daemon threads so a stuck request cannot hang the process. Then, under a per-class lock file, it creates a self-signed certificate once, reaps this user's processes holding port 8443, waits up to 10 s for the port to be free (`RuntimeError` if not), and starts `socat` as an HTTPS listener on 8443 in front of the test server (IPv4 only, own session, killed with the parent), checking it did not exit at once; a class cleanup stops it. It starts a process and writes under `/opt/hams/test/socat_certs`. **Tests:** `test_set_up_class_starts_the_tls_proxy_and_gives_the_class_a_valid_key`.
- **RealTransactionCase Class Set-Up** `[@ANCHOR: zero_sudo:real_transaction_case_setup_class]`: `setUpClass` writes the `web.base.url` and `web.base.url.freeze` rows on a separate, really committed database connection before Odoo's own set-up runs, because Odoo's set-up leaves its own uncommitted update of the same row open and a second writer would wait on it forever; then it clears the stable cache. The committed value of `web.base.url` may differ from the URL the tests use; nothing reads it back. **Tests:** `test_set_up_class_commits_the_base_url_rows_on_a_real_connection`.
- **RealTransactionCase Sign-In** `[@ANCHOR: zero_sudo:real_transaction_case_authenticate]`: `authenticate(user, password, *args, **kwargs)` signs in through Odoo's `HttpCase.authenticate` and then commits the test cursor, returning the session. The login-log row that sign-in inserts would otherwise keep a lock on the user's row for the rest of the test, and a browser tour that rewrites that login would wait the full thirty seconds. It really commits, so everything the test wrote before signing in becomes durable until the test's own clean-up. **Tests:** `test_authenticate_commits_the_test_cursor_after_signing_in`.

## Keeping the Harness Honest
- **Transaction-Impedance Hint** `[@ANCHOR: zero_sudo:monitored_test_execute]`: Wraps `TestCursor.execute`. When PostgreSQL reports a serialization failure (40001), a lock that is not available (55P03) or a unique violation (23505) inside the isolated test cursor, it logs a critical message telling the developer that a background thread, daemon or raw SQL has bypassed the test's uncommitted savepoint and that the test class should inherit from `RealTransactionCase`, then re-raises the original error. Every other result and error passes through untouched, and the call signature is forwarded unchanged because Odoo core passes keyword arguments such as `log_exceptions`.

- **Tracked Background Jobs** `[@ANCHOR: zero_sudo:tracked_threadpool_submit]` **/ Waiting for Them** `[@ANCHOR: zero_sudo:wait_for_background_futures]`: `ThreadPoolExecutor.submit` is replaced by a version that records every future, and its callable's name, until the future completes. Production code defers some work to a thread pool after commit (for example the asynchronous unpublish in `user_websites`), and that work opens its own database connection, so it can still be writing a row while a `RealTransactionCase` teardown tries to delete it. `wait_for_background_futures(timeout)` waits for all tracked jobs. It is best effort: a job still running after the timeout is named in a warning and the function returns anyway, so a job that never finishes cannot hang a test run.

## Safe Mocking
### safe_patch `[@ANCHOR: zero_sudo:safe_patch]`

**Signature.** `SafePatchMixin.safe_patch(self, target, *args, **kwargs) -> mock`, in `zero_sudo/tests/common.py`, mixed into `HamsTransactionCase`, `HamsHttpCase` and `RealTransactionCase`. A wrapper around `unittest.mock.patch(target, ...)`. `target` is the dotted path string to replace; `*args` and `**kwargs` are those of `patch` (`new`, `new_callable`, `return_value`, ...). **Behaviour.** Unless the caller passes `new` or `new_callable` (or a positional `new`), it supplies `new_callable=DiagnosticMock`. It starts the patcher, registers `self.addCleanup(patcher.stop)` so a forgotten `stop()` can never leak a patch into a later test, and returns the started mock. **Errors.** Whatever `patch` raises (a target that cannot be imported). Raw `patch` in a test file is forbidden by the burn list; this is the sanctioned way.

### safe_patch_object `[@ANCHOR: zero_sudo:safe_patch_object]`

**Signature.** `safe_patch_object(self, target, attribute, *args, **kwargs) -> mock`: the `patch.object(target, attribute, ...)` counterpart of `safe_patch`, with the same `DiagnosticMock` default, started patcher, `addCleanup(patcher.stop)` registration and returned mock. `target` is the object (class, module or instance) and `attribute` the name to replace. **Cursor guard.** It refuses outright, with `RuntimeError`, to mock anything whose type name contains `"Cursor"`: mocking a database cursor corrupts the real teardown sequence. The message points to `RealTransactionCase`, which allows real commits so a real side effect can be observed, to patching a narrower method instead, and to comparing `env.cr.sql_log_count` before and after to prove a cache hit issued no queries. **Errors.** The guard's `RuntimeError`, and whatever `patch.object` raises (a missing attribute).

### safe_patch_object_cm `[@ANCHOR: zero_sudo:safe_patch_object_cm]`

The same cursor guard and `DiagnosticMock` default as `safe_patch_object`, but it returns the unstarted patcher and registers no cleanup. It serves a patch that must end earlier than full test teardown, such as one that only needs to cover a single call driving a browser through several navigations in one profile. The caller owns the lifetime, normally by passing the result to `ExitStack.enter_context()` or using it in a `with` block. Raw `patch.object` stays forbidden in test files, and this is the sanctioned alternative.

### DiagnosticMock `[@ANCHOR: zero_sudo:diagnostic_mock_init]`

`class DiagnosticMock(MagicMock)` is the default mock of `safe_patch`. `__init__(self, *args, **kwargs)` takes the arguments of `MagicMock` plus the keyword `max_recursion_depth` (int, default 5), which it removes before calling the parent constructor, and records it as `_max_depth`; `_current_depth` starts at 0. Both are per instance and are not thread-safe, which is acceptable because a test patches one thing at a time. The class exists so that a mock that accidentally calls itself (a common copy-paste mistake when patching a method that recurses) stops loudly, instead of silently spinning or reaching the real Python recursion limit; see its call entry.

### DiagnosticMock Recursion Guard `[@ANCHOR: zero_sudo:diagnostic_mock_call]`

`DiagnosticMock.__call__(self, *args, **kwargs)` increments `_current_depth`; if it then exceeds `_max_depth` it resets the depth to 0 and raises `RecursionError("DiagnosticMock Security Trip: Recursion depth limit (N) exceeded.")`. Otherwise it calls `MagicMock.__call__` and returns its result (so `return_value` and `side_effect` work as usual), decrementing the depth in a `finally` whether the call returned or raised. Only nested calls (the mock called from inside its own side effect) can reach the limit; sequential calls do not. The error is for the developer: fix the patch, do not raise `max_recursion_depth` to make it pass.

## Synthetic Callsigns
### generate_test_callsign `[@ANCHOR: zero_sudo:generate_test_callsign]`

**Signature.** `generate_test_callsign(prefix="T") -> str`. **Returns** `f"{prefix}{n:04d}X"`, where `n` comes from the module's `itertools.count(1)` (`T0001X`, `T0002X`, ...), so every call in one process returns a value no other call can repeat and no real callsign can collide with it (the values are not valid allocations). **Why.** Ham-radio fixtures in both repositories need a unique valid-looking callsign per record without a `UniqueViolation` on a real one. The counter is strictly increasing within a process and starts again at 1 in each new process, so a collision can only mean two separate OS processes sharing one database (a bypassed test-runner lock), not two test classes. **Errors.** None. Thread-safe enough in CPython (`next` on `itertools.count`). Tests: `test_tdd_fixes.py`.

### get_callsign `[@ANCHOR: zero_sudo:get_callsign]`

**Signature.** `SafePatchMixin.get_callsign(cls, key="W1AW") -> str` (a classmethod). **Returns** a synthetic callsign cached per test class and key: the first call for a `(class, key)` pair stores `generate_test_callsign()` in the class's `_callsign_map` dict and later calls return the identical value, so several methods of one class can refer to "the club callsign" by key without hard-coding a real one. `key` is any hashable label (it does not appear in the callsign). The map lives on the class and is not cleared between tests, which is the intended reuse; a test that needs a fresh callsign calls `generate_test_callsign` directly. **Errors.** None. Not thread-safe, like the rest of the harness. Tests: `test_tdd_fixes.py`.

## Secure Daemon RPC
### SecureJSONRPCClient.__init__ `[@ANCHOR: zero_sudo:json_rpc_client_init]`

**Signature.** `SecureJSONRPCClient(env_path, base_url, db_name="hams")`, in `zero_sudo/daemon/json_rpc_client.py` (Odoo-free; used by external daemons). The client talks to Odoo's JSON-2 interface with an API key.

| Argument | Type | Meaning |
|---|---|---|
| `env_path` | `str` | Path of the `.env` file a `daemon_key_manager` run wrote, holding `ODOO_RPC_LOGIN=` and `ODOO_RPC_KEY=`. Never a hard-coded secret. |
| `base_url` | `str` | Odoo base URL; a trailing `/` is removed. |
| `db_name` | `str` | Database sent in the `X-Odoo-Database` header. |

The constructor stores them, creates a `requests.Session`, and calls `_load_credentials` at once, so a client cannot exist without credentials. **Errors.** `FileNotFoundError` and `ValueError` from `_load_credentials`. `call(model, method, ids=(), **kwargs)` posts to `/json/2/<model>/<method>` with `Authorization: Bearer <key>` (30 s timeout), retries once after reloading the credentials on HTTP 401 or 403 (a rotated key), and raises `RuntimeError` on a non-JSON answer or a JSON `error`. The key is held in memory and sent as a bearer token; use https.

### SecureJSONRPCClient._load_credentials `[@ANCHOR: zero_sudo:json_rpc_client_load_credentials]`

`_load_credentials(self)` reads the `.env` file at `self.env_path` line by line and sets `self.login` from the line `ODOO_RPC_LOGIN=...` and `self.api_key` from `ODOO_RPC_KEY=...` (the value is everything after the first `=`, stripped of the line ending; later lines win). **Errors.** `FileNotFoundError` with a message that the daemon must be registered and the key-manager cron must have run when the file does not exist; `ValueError` ("Malformed credential file ... Missing LOGIN or KEY") when either value is absent or empty. It is called from the constructor and again by `call` after a 401 or 403, which is how a real key rotation mid-run is healed without restarting the daemon. It reads a file only; the key is never logged.
