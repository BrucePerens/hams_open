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

**Purpose.** The base class for HTTP, tour (UI) and browser tests: `class HamsHttpCase(HttpCase, SafePatchMixin, PersonaMixin)` in `zero_sudo/tests/common.py`. It keeps Odoo's `HttpCase` isolation (the server thread and the test share one `TestCursor` over the class's savepoint transaction, so nothing a test writes survives it) and adds this project's setup and teardown, a TLS proxy, the Chrome pool and the browser patches described below.

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

- **Reaping Leaked Headless Chrome** `[@ANCHOR: zero_sudo:kill_all_headless_chrome]` **/ Which Browsers Are This Run's** `[@ANCHOR: zero_sudo:headless_chrome_of_this_run]`: Kills every headless `chrome`, `chromium` or `chromium-browser` process of the current uid that belongs to THIS test process: a descendant of it, or an orphan whose `--user-data-dir` is a directory under this process's `tempfile.gettempdir()` that exists in its own view of the filesystem (what a tour whose parent died leaves). It runs before a new Chrome is spawned and at the end of every test, so memory held by a tour that did not tear down completely is reclaimed at once. It used to kill every headless browser of the uid on the machine, which was safe only while `test.py` allowed one run at a time; with test slots (ADR 0116) several runs share the machine, a run in its own PID namespace is visible in the host's process table, and the old reap killed other runs' browsers mid-tour (exit code 9, `CancelledError` in `Page.navigate`, a stalled tour step). A non-headless browser, another user's process, any process that is not one of those three browsers, and another run's browser are left alone. The function name keeps `_for_this_uid` for history; `_headless_chrome_of_this_run` returns the processes (psutil objects) without killing them.

- **Hypothesis Profile for Property Tests** `[@ANCHOR: zero_sudo:hypothesis_profile]`: Every property test that runs in an Odoo test process (a `@given` test in `ham_com`, `ham_base`, `ham_dns`, `ham_propagation`, `ham_repeater_dir`, `ics_forms`, `edge_routing`) runs under one registered Hypothesis settings profile, `hams_odoo_tests`, so a loaded machine cannot fail it. Found 2026-11-08: the whole-system gate ran at load 100 and `TestForgivingCallsignDomainProperties.test_domain_with_no_callsign_field_is_unchanged` failed with `FailedHealthCheck: Input generation is slow: Hypothesis only generated 1 valid inputs after 38.03 seconds`, a timing check and not a correctness one.
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

- **Pool** `[@ANCHOR: zero_sudo:chrome_pool]` **/ Leased Chrome** `[@ANCHOR: zero_sudo:pooled_chrome]` **/ Pool Error** `[@ANCHOR: zero_sudo:chrome_pool_error]`: `ChromePool(start, max_uses=40)` (`[@ANCHOR: zero_sudo:chrome_pool_init]`; its `chrome` property, `[@ANCHOR: zero_sudo:chrome_pool_chrome]`, is the live Chrome or `None`; `PooledChrome.__init__` is `[@ANCHOR: zero_sudo:pooled_chrome_init]`) holds at most one `PooledChrome` (the Popen object, DevTools port, profile directory, the test class it was started for, whether touch events are on, and how many tests it has served). `start(touch_enabled)` is supplied by the harness and returns `(proc, devtools_port, user_data_dir, stop)`. Odoo runs the tests of a process one after the other, so there is no concurrent lease; a lock guards the state anyway. `ChromePoolError` is raised when a DevTools command to the Chrome returns an error or no answer within its timeout; callers treat it as "this Chrome is not usable".
- **Lease** `[@ANCHOR: zero_sudo:chrome_pool_acquire]`: `acquire(owner, touch_enabled)` returns the live Chrome for a test of class `owner` (the test case's class), starting one when there is none. A Chrome is stopped and replaced first when it was started for another class, needs other launch switches (touch events are a switch, not a per-page setting), has served `max_uses` tests (bounds what a long class accumulates), is not running, or its DevTools endpoint does not answer. It increments `uses`. Errors from `start` (a Chrome that cannot be started raises `unittest.SkipTest`, as core's does) propagate and leave the pool empty.
- **Fresh Page** `[@ANCHOR: zero_sudo:chrome_pool_new_page]` **/ Dispose** `[@ANCHOR: zero_sudo:chrome_pool_dispose_context]` **/ End of Page** `[@ANCHOR: zero_sudo:chrome_pool_end_page]`: `new_page(chrome)` creates a browser context and a blank page in it and returns `(context_id, page_websocket_url)`; a failure after the context exists disposes it before raising. `end_page(chrome, context_id)` disposes the context and returns `False` when that failed (a crashed or hung Chrome), which the caller uses to mark the test unclean. Both talk to the browser (not the page) over a short-lived websocket (`PooledChrome.command`).
- **Release** `[@ANCHOR: zero_sudo:chrome_pool_release]`: `release(chrome, clean)` ends a lease. A clean test leaves the Chrome in the pool; an unclean one (an exception left the browser's start-up or body, `had_failure`, a failed result, a context that could not be disposed) stops it, so the next test starts a fresh Chrome and a crashed or hung Chrome fails only the test that used it. Releasing a Chrome the pool no longer holds (it was already replaced) never stops its successor.
- **Close** `[@ANCHOR: zero_sudo:chrome_pool_close]`: stops the live Chrome. Called by `HamsHttpCase.tearDownClass`, at interpreter exit (`atexit`), before a browser that is not pooled spawns its own Chrome (its spawn reaps every browser of the run), and by `acquire`/`release` when they replace a Chrome. Safe to call with nothing running.
- **What Belongs to the Pool** `[@ANCHOR: zero_sudo:chrome_pool_owned_pids]` **/ By Profile** `[@ANCHOR: zero_sudo:chrome_pool_owns_profile]`: the pids of the live Chrome and of everything below it, and whether a profile directory is the live Chrome's. `_headless_chrome_of_this_run` (the end-of-test reaper) leaves those alone; otherwise the reaper would kill the class's Chrome after every test.
- **Chrome Health, Output and Stop** `[@ANCHOR: zero_sudo:pooled_chrome_alive]` `[@ANCHOR: zero_sudo:pooled_chrome_pids]` `[@ANCHOR: zero_sudo:pooled_chrome_log_size]` `[@ANCHOR: zero_sudo:pooled_chrome_log_since]` `[@ANCHOR: zero_sudo:pooled_chrome_command]` `[@ANCHOR: zero_sudo:pooled_chrome_stop]`: `alive()` is true while the process runs and `/json/version` answers within 3 s. `pids()` lists the process tree. `log_size()` and `log_since(offset)` give the part of `chrome_debug.log` a test wrote (core's per-test log file is the one it saves on failure). `command(method, params, timeout)` runs one browser-level DevTools command and returns its result, raising `ChromePoolError` on an error reply or a missing answer. `stop()` runs the harness's stop (the same termination and process-tree kill every test Chrome gets) and removes the profile directory; calling it twice is safe.
- **Starting the Pool's Chrome** `[@ANCHOR: zero_sudo:start_pooled_chrome]` **/ Profiles on a Command Line** `[@ANCHOR: zero_sudo:profiles_of]`: `_start_pooled_chrome(touch_enabled)` is the pool's `start`: it runs `_patched_chrome_start` (own process group, PID namespace, reaper protections) on a stand-in `ChromeBrowser` and returns `(proc, devtools_port, user_data_dir, stop)`; the profile directory is removed again when start-up fails. `_profiles_of(process)` returns the `--user-data-dir` values of a psutil process's command line, which the reaper compares with the pool's profile.
- **When the Pool Applies** `[@ANCHOR: zero_sudo:pool_applies]`: a browser uses the pool unless it is the pool's own start-up, visible (`watch`) or a debugging browser, a `--pause-on-fail` run (the fixed DevTools port 9222), an `--offline-isolation` run (a Chrome in its own network namespace, `HAMS_OFFLINE_NS_CTL`), or `HAMS_CHROME_POOL=0`.
- **Pooled Start** `[@ANCHOR: zero_sudo:pooled_chrome_start]` **/ Clean Test** `[@ANCHOR: zero_sudo:pooled_test_was_clean]`: `_pooled_chrome_start` replaces core's `_chrome_start` for a pooled browser: it leases the class's Chrome, records where its log stood, yields the process and port core expects, and on exit releases it with `_pooled_test_was_clean(browser)` (no exception, no `had_failure`, no failed `_result`, context disposed).
- **Page Websocket** `[@ANCHOR: zero_sudo:patched_open_websocket]` **/ Browser Close** `[@ANCHOR: zero_sudo:patched_websocket_request]` **/ Chrome Log** `[@ANCHOR: zero_sudo:patched_read_log]`: for a pooled browser core's websocket step opens the new context's page instead of the one blank page of a private Chrome, and closes the websocket and disposes the context at the end; `Browser.close`, core's last shutdown step, is swallowed (it would end the class's Chrome); `read_log` returns the test's share of the Chrome's log. An unpooled browser takes core's path unchanged.

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
- **safe_patch** `[@ANCHOR: zero_sudo:safe_patch]` **/ safe_patch_object** `[@ANCHOR: zero_sudo:safe_patch_object]`: Thin wrappers around `unittest.mock.patch`/`patch.object` that auto-register `addCleanup(patcher.stop)` (so a forgotten `.stop()` can never leak a patch into a later test) and, unless the caller passes its own `new`/`new_callable`, default to `DiagnosticMock` instead of a bare `MagicMock`.

- **safe_patch_object_cm** `[@ANCHOR: zero_sudo:safe_patch_object_cm]`: The same cursor guard and `DiagnosticMock` default as `safe_patch_object`, but it returns the unstarted patcher and registers no cleanup. It serves a patch that must end earlier than full test teardown, such as one that only needs to cover a single call driving a browser through several navigations in one profile. The caller owns the lifetime, normally by passing the result to `ExitStack.enter_context()` or using it in a `with` block. Raw `patch.object` stays forbidden in test files, and this is the sanctioned alternative.

- **DiagnosticMock** `[@ANCHOR: zero_sudo:diagnostic_mock_init]` **/ Recursion Guard** `[@ANCHOR: zero_sudo:diagnostic_mock_call]`: A `MagicMock` subclass that trips a `RecursionError` past a configurable depth (default 5) -- catches a mock that's accidentally calling itself (a common copy-paste mistake when patching a method that recurses) loudly, rather than letting it silently spin or blow the real Python recursion limit.

- **safe_patch_object's Cursor Guard**: `safe_patch_object` refuses outright to mock anything whose type name contains `"Cursor"` -- mocking a database cursor corrupts the real test teardown sequence; `RealTransactionCase` is the documented alternative when a real side effect needs to be observed instead of mocked.

## Synthetic Callsigns
- **Callsign Generator** `[@ANCHOR: zero_sudo:generate_test_callsign]` **/ Cached Accessor** `[@ANCHOR: zero_sudo:get_callsign]`: Ham-radio test fixtures across both repos need a unique, valid-looking callsign per test without colliding on a real one -- `generate_test_callsign` hands out a monotonically-increasing synthetic value (`T0001X`, `T0002X`, ...), and `get_callsign(key)` caches one per class+key so repeated calls with the same key return the identical value within a test.

## Secure Daemon RPC
- **SecureJSONRPCClient Init** `[@ANCHOR: zero_sudo:json_rpc_client_init]` **/ Credential Loading** `[@ANCHOR: zero_sudo:json_rpc_client_load_credentials]`: External daemons authenticate to Odoo over JSON-2 IPC using credentials read strictly from a local `daemon_key_manager`-managed `.env` file (never a hardcoded secret) -- a missing file raises `FileNotFoundError`, a malformed one (missing login or key) raises `ValueError`, and a real key rotation mid-run triggers this client's own self-healing retry rather than a hard failure.
