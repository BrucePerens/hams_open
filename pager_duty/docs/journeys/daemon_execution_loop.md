# Journey: Daemon Execution Loop

This journey tracks the flow of data from Odoo configuration to the standalone monitoring daemon.

## 1. Configuration in Odoo
- **Entry:** Admin navigates to Monitoring Checks (the views are described in the first entry below).
- **Schema:** Data is stored in the `pager.check` model.

### Monitoring Checks and incident views render [@ANCHOR: test_pager_view]

This anchor marks the test that proves the views an administrator opens in this journey can be built. `test_04_views_render` in `tests/test_incident.py` calls `get_view` for the `pager.incident` form and list views and for the `calendar.event` form, and `test_03_views_render` in `tests/test_generalized_config.py` does the same for the `pager.check` form and list. Each call must return a dict with an `arch` key, so a view that names a missing field or has invalid XML fails the test. The views in `views/incident_views.xml` cite the anchor in their `audit-ignore-view` comments instead of carrying a test of their own. Arguments: none (a view test takes no input and has no return value beyond pass or fail). Errors: `get_view` raises for a broken view. It needs a database and runs as a post-install test.

## 2. Sync to Daemon
- **Wizard:** The admin uses the JSON configuration tools in Monitoring Checks [@ANCHOR: generalized_pager_config].
- **Export:** `action_push_to_json()` transforms ORM records into a JSON structure.
- **Persistence:** The file is written to the daemon's local filesystem (e.g., `pager_duty/daemon/pager_config.json`, or the directory named by the `pager_duty.config_dir` system parameter when it exists and is writable; see "Config Isolation" in `../../README.md`).

## 3. Execution Cycle
- **Boot:** `generalized_monitor.py` starts and parses the JSON.
- **Dependency Check:** It verifies required system binaries before any check thread starts.

### `verify_and_install_dependencies(client, checks)` [@ANCHOR: daemon_verify_dependencies]

Called once at boot from `main()` of `daemon/generalized_monitor.py`. It maps each check type in the configuration to the one binary that type shells out to (`dns` to `dig`, `snmp` to `snmpget`, `pg_dump`, `nginx`, `certbot`, `logrotate`, `http3` to `curl`, `icmp` to `ping`, `docker`, `systemd` to `systemctl`, `cloudflared`) and collects the set of binaries needed. For each one that `shutil.which` cannot find, it asks Odoo to provision it.

| Argument | Type | Meaning |
|---|---|---|
| `client` | `OdooClient` | The shared JSON-2 client. Borrowed; `execute("pager.check", "rpc_ensure_executable", cmd_name=cmd)` is called on it. |
| `checks` | `list[dict]` | The parsed check list (each with a `type` key). Read only. |

Returns nothing. On a successful provision the directory of the returned `path` is prepended to `PATH` for this process. The provisioning call is retried 12 times, 10 seconds apart (a blocking `time.sleep`, in the main thread, at boot only), because Odoo may still be starting. If a binary cannot be provisioned it logs a critical message, calls `fallback_notify("Daemon Boot", ..., "critical")`, tries to report a critical "Daemon Boot" incident through RPC (an RPC failure is only logged) and ends the process with `sys.exit(1)`, so systemd restarts it. The exit is inside the `if not success` branch, so a provisioned dependency never stops the daemon. Tests: The anchor is cited by `test_10_certbot_check` and `test_23_synthetic_check_extracts_graduated_severity_from_stderr` in `tests/test_generalized_monitor.py`.

- **Watchdog:** The main thread starts an execution thread for each check and monitors their heartbeats [@ANCHOR: daemon_main_loop]. (These are liveness signals each polling thread records for the watchdog; they are unrelated to the "Heartbeat (Push Monitor)" check type, where a remote job pings Odoo.)

- **Isolation:** Each check type (HTTP, XML-RPC, Heartbeat) runs in its own isolated logic block (next entry).

### `execute_check(check, client=None)` [@ANCHOR: daemon_execute_check]

The one function that decides whether a single check passes. `polling_thread` and `check_once` call it every cycle. It reads `check["type"]` and resolves `check["target"]` through `parse_env`, then runs one `if/elif` branch per type (`system`, `load`, `ftp`, `imap`, `pop3`, `mysql`, `ldap`, `ntp`, `snmp`, `dns`, `http`, `http3`, `tcp`, `udp`, `redis`, `rabbitmq`, `xmlrpc`, `jsonrpc`, `postgres`, `anomaly`, `ssl`, `synthetic`, `certbot`, `pg_dump`, `nginx`, `logrotate`, `cloudflared`, `smtp_dryrun`, `icmp`, `docker`, `memcached`, `ssh`, `heartbeat`, `smart`, `playwright`/`bash`/`executable`, `file_absent`, `freshness`, `systemd`), each using only the keys that type needs (`partition`, `critical`, `port`, `user`, `password`, `regex` and so on).

| Argument | Type | Meaning |
|---|---|---|
| `check` | `dict` | One entry of `pager_config.json`. Borrowed, not modified. |
| `client` | `OdooClient` or `None` | Used by the few types that ask Odoo for something (the freshness feed check). `None` for `--check-once`. |

Returns `(success: bool, message: str)`; the message is `"OK"` or a text naming the failure, and a synthetic script may prefix `[SEVERITY:high] ` (see `extract_severity_prefix`). Errors: each branch catches its own network, subprocess and parse errors and returns `(False, text)` rather than raising, so one broken check never ends its thread. It blocks for the check's timeout (typically 5 to 15 seconds). It holds no lock and writes no state itself. Credentials come from the check or from the environment and are never put into the message. Tests: `tests/test_generalized_monitor.py` (`test_03` to `test_15`) and `tests/test_never_run_and_monitor_hardening.py`.

- **Failover:** If Odoo is unreachable, the daemon falls back to direct `SMTP` or `Webhook` alerts.

### `report(client, source, msg, severity="high", website_id=False, units=())` [@ANCHOR: daemon_report_incident]

Raises one incident. It first posts a Discord-style JSON message to `PAGER_WEBHOOK_URL` when that variable is set (5-second timeout, any failure only logged), then calls `pager.incident.report_incident` through `client.execute` with `vals` holding `source`, `description` (the message), `severity`, and `website_id` and `units` when present. If the RPC raises anything, it logs the error and calls `fallback_notify(source, msg, severity)` so the page still goes out by e-mail.

| Argument | Type | Meaning |
|---|---|---|
| `client` | `OdooClient` | Shared client. Borrowed. |
| `source` | `str` | The check name; Odoo groups repeated reports by source. |
| `msg` | `str` | The failure text, stored as the description. |
| `severity` | `str` | `low`, `medium`, `high` or `critical`; default `high`. |
| `website_id` | `int` or `False` | The check's website, sent only when set. |
| `units` | iterable of `str` | systemd unit names behind the failure, sent as a list when not empty. |

Returns nothing and does not raise. It blocks for up to the webhook timeout plus the RPC timeout (15 s) plus the SMTP timeout (10 s). It is called from check threads and holds no lock. The webhook URL and SMTP credentials come from the environment. Tests: `test_01_smtp_fallback` in `tests/test_generalized_monitor.py`.

## 4. Feedback Loop
- **Status Reporting:** Results are pushed back to Odoo via its JSON-2 API (`/json/2/<model>/<method>`, see section 5 below and `../../daemon/README.md`). Two writes happen: `record_check_status` updates the check's own row, and a confirmed failure goes through `report` [@ANCHOR: daemon_report_incident], documented above (the same function serves the failover path). Every confirmed failure becomes or updates one `pager.incident`; a pass never creates one.

- **Dashboard:** The NOC Board reflects the latest check statuses in real time.

### `get_board_data()` of `pager.incident` [@ANCHOR: pager_board_data]

The server call behind the NOC Board (`models/incident.py`, a `@public_rpc` `@api.model` method). Arguments: none besides the Odoo environment; the website comes from `context["website_id"]`, else from `website.get_current_website()`, and the company is `env.company`. It runs `SELECT pager_get_board_data(website_id, company_id)`, a PostgreSQL function installed by `hooks.py`, so the whole board is one round trip. Returns a dict (JSON from the function): `on_duty` (name of the user whose pager-duty calendar event covers now, or the text `"None"`), `active` (up to 50 open or acknowledged incidents, newest first, each with `name`, `source`, `severity`, `status`, `create_date`, `ack_name`), `resolved` (the 10 most recently resolved) and `stats` (the number of checks per status, built in the same method). Rows are filtered to the website and company, so one tenant never sees another's incidents. Errors: a database error propagates; if the function is missing the call fails. No locks, no writes. The bus message `update_board` makes open boards call it again. Tests: `test_01_board_data_rpc` in `tests/test_board_edge_cases.py`, `test_07_board_data_procedure` in `tests/test_incident.py`.

## 5. The RPC Client and Bootstrap

### `parse_env(val)` [@ANCHOR: parse_env]

Resolves one configuration value that may name an environment variable. If `val` is a string beginning with `ENV:`, the rest is looked up with `os.environ.get` and returned (an empty string when the variable is unset or empty); any other value, including non-strings, is returned unchanged. A check stored in Odoo can therefore say `ENV:DB_PASS` for a password, `ENV:REDIS_HOST` for a target or a partition, so the secret lives in the daemon's environment file and never in the database or `pager_config.json`. Arguments: `val` (any, borrowed). Returns the resolved value; never raises. No I/O, safe from any thread. It does not parse the daemon's own startup variables (Odoo URL, database, key): `get_odoo_client` reads those directly. Test: `test_02_parse_env_util` in `tests/test_generalized_monitor.py`.

### `get_odoo_client(logger, config)` [@ANCHOR: get_odoo_client]

Builds the single `OdooClient` every thread shares. The URL is `ODOO_URL` (default `http://odoo:8069`). The database is `config["odoo_database"]`, else `ODOO_DB`, else the first entry of Odoo's `/web/database/list` (5-second timeout, failures only logged), else the literal `odoo`. The login name (`ODOO_USER`, else `ODOO_RPC_LOGIN`, else `pager_service_internal`) is accepted but not sent. The bearer key is `ODOO_PASSWORD`, else `ODOO_RPC_KEY` (the name `daemon_key_manager` writes).

| Argument | Type | Meaning |
|---|---|---|
| `logger` | `logging.Logger` | Receives warnings and the critical message. |
| `config` | `dict` | The parsed configuration; only `odoo_database` is read. |

Returns an `OdooClient`, or `None` when neither key variable is set (critical log: "refusing to start"; the caller halts rather than run with paging silently disabled) or when construction fails. No network call is made for the key itself. Tests: `test_19_get_odoo_client_reads_env_and_falls_back_to_odoo_db` and tests in `tests/test_never_run_and_monitor_hardening.py`.

### `OdooClient.__init__(url, db, user, password)` [@ANCHOR: odoo_client_init]

Creates the JSON-2 client (class `OdooClient` in `daemon/generalized_monitor.py`). It strips a trailing `/` from `url`, keeps `db`, and builds the header dict used by every call: `Authorization: bearer <password>`, `X-Odoo-Database: <db>`, `Content-Type: application/json` and `User-Agent: Pager-Daemon/1.0`.

| Argument | Type | Meaning |
|---|---|---|
| `url` | `str` | Odoo base URL, no path. |
| `db` | `str` | Database name sent in `X-Odoo-Database`. |
| `user` | `str` | Accepted and not stored; the key identifies the account. |
| `password` | `str` | The API key, sent as the bearer token. Secret: never logged. |

Returns the instance; it makes no network call and does not raise. The header dict is mutated later by `reload_key` when a rotated key is picked up, so the client is shared by all threads and its header dict is replaced in place. Test: `test_18_odoo_client_execute_posts_json2_and_parses_the_response`.

- **JSON-2 Transport, continued:** `OdooClient.execute()` [@ANCHOR: odoo_client_execute] is the one method every check thread calls to actually make an RPC -- it POSTs to Odoo's `/json/2/<model>/<method>` endpoint and parses the JSON response, so the rest of the daemon never touches `urllib` directly. Its arguments are `model`, `method` and keyword arguments sent as the JSON body; it uses an opener that refuses redirects (so the key is never sent to another host), a 15-second timeout, and on an HTTP 401 re-reads the key file once through `reload_key` and sends the call again. Any other HTTP error raises `Exception("JSON-2 API Error <code>: <body>")`.

- **Dependency Verification:** Before trusting a check that shells out to a binary (`curl`, `ping`, a synthetic script), the daemon confirms the binary is actually executable on this host, so a missing dependency fails with a clear message rather than a confusing subprocess error deep in a check thread.

### `ensure_executable(cmd_name)` [@ANCHOR: ensure_executable]

Looks one binary up on `PATH` with `shutil.which`. Argument: `cmd_name` (`str`, a bare command name, not a path or command line). Returns a tuple: `(path, "")` when found, or `(None, "Missing dependency: '<name>'. Startup verification failed.")` when not. It does not raise, does not execute the binary and does not provision it (that is `verify_and_install_dependencies`). It is a quick filesystem scan, safe from any thread, and has no side effects. The check branches for `systemd`, `snmp`, `dns`, `http3` and similar types call it first and return the message as the failure text. Test: `test_09_process_wrappers` in `tests/test_generalized_monitor.py`.

## 6. Per-Check Execution Threads

- **Maintenance Windows:** Before running a check's logic, the thread asks whether the check is inside a configured maintenance window [@ANCHOR: is_in_maintenance] (distinct from the host-wide pagerduty maintenance flag described below) -- if so, the thread skips alerting entirely for that cycle, so planned maintenance doesn't page anyone. `is_in_maintenance(check)` reads the `maint_start` and `maint_end` strings (`YYYY-MM-DD HH:MM:SS`, UTC) and returns `True` only when both are present and now lies between them; a missing or unparsable value returns `False` (a parse error is logged as a warning) so a bad window can never hide a check.

### `polling_thread(client, check)` [@ANCHOR: polling_thread]

The thread body that runs one polled check forever; `main()` starts one per non-log check. Settings read from `check`: `name`, `id` (the state key, via `check_key`), `website_id`, `interval` (seconds, default 60), `grace` (seconds after start during which a confirmed failure is only logged), `threshold` (consecutive failures before reporting, at least 1), `parent`/`parent_id` and `remediate`.

| Argument | Type | Meaning |
|---|---|---|
| `client` | `OdooClient` | Shared; borrowed. |
| `check` | `dict` | One configuration entry. Read only. |

Never returns. Each pass records a heartbeat in `THREAD_HEARTBEATS[key]`, runs `execute_check`, writes the status with `record_check_status`, and on success clears the failing flags and calls `auto_resolve` after three passes in a row. A failure under the active pagerduty maintenance flag is not counted. Otherwise failures are counted; below the threshold the check is re-run after at most 20 seconds, at the threshold the check is marked failing and `report` is called (unless inside the grace period), and a `remediate` script that exists runs once per failure streak. The first pass runs at once, then the thread sleeps a random start-up jitter, then loops, skipping a pass while `is_in_maintenance` or `parent_suppresses` is true. Shared dicts (`THREAD_HEARTBEATS`, `FAILING_CHECKS`) are written only by their own thread and read by the watchdog; no lock is held. Tests: `test_20_polling_thread_runs_one_real_cycle_then_exits` and the `test_20b` to `test_20d` tests.

### `log_tail_thread(client, check)` [@ANCHOR: log_tail_thread]

The thread body for a `log` check: it follows `check["target"]` (the file) like `tail -F` and reports any new line matching `check["regex"]` (case-insensitive) as a `critical` incident through `report`. Arguments: `client` (`OdooClient`, borrowed) and `check` (`dict` with `name`, `id`, `website_id`, `target`, `regex`, `grace`). Never returns. On the first open it seeks to the end of the file, so history is never re-reported; when the inode changes (rotation) it reopens and reads the new file from the start. It records a heartbeat each pass and sets its watchdog timeout to 120 seconds. It sleeps 0.5 s when idle and 5 s after any `OSError` (a missing or unreadable file is retried, never fatal). Matches inside the `grace` period after start are logged but not reported. It does not truncate-detect (the log analyzer does that), and it does not honour the pagerduty maintenance flag. The file is opened with the daemon's own user. Test: `test_21_log_tail_thread_runs_one_real_cycle_then_exits`.

- **SMTP/Webhook Fallback:** If the Odoo RPC itself is unreachable, `fallback_notify()` [@ANCHOR: fallback_notify] sends the alert directly via SMTP or a webhook instead, so a down Odoo instance doesn't also mean a silent monitoring fleet. While the pagerduty maintenance flag is active (set by an operator with `pagerduty-maintenance start`, read by [@ANCHOR: pager_duty:maintenance_read_flag]), `fallback_notify()` logs a warning and sends nothing; an expired, over-long or malformed flag (including one whose `set_at` or `until` is outside 1970..2100) is ignored, so paging stays on.

- **Pagerduty maintenance flag:** an operator marks a planned restart with `pagerduty-maintenance start` (`sudo`; `end` and `status` are the other two commands). The flag is one JSON file, `{"set_at": <UTC epoch>, "until": <UTC epoch>, "reason": "...", "set_by": "..."}`, written by root with mode 0644 so the read-only monitor units can read it; writing needs root on purpose. The functions are in `daemon/pagerduty_maintenance.py` (standard library only) and are documented one by one below. Rules enforced when reading: a flag expires by itself, `until` is capped at `set_at` plus 120 minutes, and a malformed flag counts as NOT in maintenance so a damaged file can never silence paging.

### `flag_path(env=None)` [@ANCHOR: pager_duty:maintenance_flag_path]

Returns where the flag file lives: the value of `PAGERDUTY_MAINTENANCE_FILE` in the environment, else `/etc/pagerduty/maintenance`. Argument: `env` (a mapping, default `os.environ`; tests pass a dict). Returns a `str` path. No file access, never raises. The environment override exists so tests and the `--file` option can point elsewhere; `hams_shared/tools/site_monitor.py` carries a read-only copy of the same rule, and a test in `pager_duty/tests` fails if they disagree.

### `read_flag(path=None, now=None)` [@ANCHOR: pager_duty:maintenance_read_flag]

Reads and classifies the flag. Arguments: `path` (default `flag_path()`), `now` (epoch seconds, default the current time; tests pass a fixed value). Returns a dict whose `state` is `absent` (no file), `malformed` (unreadable, not JSON, `set_at` or `until` missing, not finite, outside 1970..2100, or `set_at` more than 300 seconds in the future; the dict then has an `error` text), `expired` or `active`. For the last two it also has `until` (already capped at `set_at` + 7200), `remaining` seconds, `reason` and `set_by`. It never raises and never writes, so the unprivileged monitors under `ProtectSystem=strict` can call it every pass. Called by `fallback_notify`, `log_anomaly_in_maintenance` and `polling_thread`. Test: `tests/test_pagerduty_maintenance.py`.

### `is_active(path=None, now=None)` [@ANCHOR: pager_duty:maintenance_is_active]

The yes/no form of `read_flag`: returns `True` only when its `state` is `active`, so absent, expired and malformed all give `False` (paging stays on). Arguments are as for `read_flag`. Never raises, read only, no side effects. The daemon itself calls `read_flag` so it can log a malformed flag; `is_active` is the form for a caller that needs only the decision. Test: `tests/test_pagerduty_maintenance.py`.

### `remove_expired(path=None, now=None)` [@ANCHOR: pager_duty:maintenance_remove_expired]

Deletes the flag file when, and only when, it has expired. Arguments as for `read_flag`. Returns `True` if the file was removed, `False` for an absent, active or malformed flag (a malformed file is left for a person to see) and also when `os.unlink` fails (for example an unprivileged `status` call). Called by the `start` and `status` commands; the monitors never call it, because they run with a read-only `/etc`. It makes no lock; two commands racing see at worst one `False`.

### `write_flag(minutes=20, reason="", set_by="", path=None, now=None)` [@ANCHOR: pager_duty:maintenance_write_flag]

Creates the flag atomically. `minutes` must be 1 to 120 (else `ValueError`); `reason` and `set_by` are free text stored as given; `path` and `now` as for `read_flag`. It creates the directory (mode 0755) if needed, writes a temporary file `<path>.tmp.<pid>` opened with `O_EXCL` and mode 0644, then `os.replace`s it over the flag, so a reader never sees a half-written file, and removes the temporary on failure. Returns the dict written (`set_at`, `until`, `reason`, `set_by`). Raises `OSError` when the caller cannot write, which in practice means not root; that is deliberate, so an unprivileged user cannot silence paging.

### `clear_flag(path=None)` [@ANCHOR: pager_duty:maintenance_clear_flag]

Removes the flag whatever its state. Argument: `path` (default `flag_path()`). Returns `True` when a file was there and is now gone, `False` when none existed. Other `OSError`s, such as a permission failure, propagate and `main` reports them with exit status 2. Used by `pagerduty-maintenance end`. It ends maintenance at once; paging resumes on the monitors' next pass.

### `_fmt_time(epoch)` [@ANCHOR: pager_duty:maintenance_fmt_time]

Formats an epoch as `YYYY-MM-DD HH:MM:SS UTC` with `time.gmtime`. Argument: `epoch` (seconds, a number). Returns the text. It can only be given the `until` value of `read_flag`, which refuses anything outside 0..MAX_EPOCH, so `gmtime` cannot overflow. No side effects. Used by `describe` and the `status` message for an expired flag.

### `_fmt_remaining(seconds)` [@ANCHOR: pager_duty:maintenance_fmt_remaining]

Formats a time left as `<minutes>m<seconds:02d>s`, for example `19m05s`. Argument: `seconds` (a number, truncated to an int and clamped to at least 0, so an overrun shows `0m00s`). Returns the text. Pure function, no I/O. Used by `describe`.

### `describe(info)` [@ANCHOR: pager_duty:maintenance_describe]

One line for logs and the command output, built from an `active` or `expired` dict returned by `read_flag`: `until <time> (<m>m<ss>s left)`, then `, reason: <reason>` and `, set by <user>` when those are not empty. Argument: `info` (dict with `until` and `remaining`; `reason` and `set_by` optional). Returns `str`; raises `KeyError` if `until` or `remaining` is missing, which cannot happen for a dict from `read_flag`. The daemon puts this text into its maintenance log lines.

### `main(argv=None, now=None, out=None)` [@ANCHOR: pager_duty:maintenance_main]

The `pagerduty-maintenance` command. Arguments: `argv` (list, default `sys.argv[1:]`), `now` (epoch, for tests), `out` (stream for normal output, default stdout). Subcommands: `start [--minutes N] [--reason TEXT]` (N 1 to 120, default 20; removes an expired flag, records the invoking user from `SUDO_USER` or `USER`, writes the flag, prints its description), `end` (clears the flag) and `status` (prints the state). A global `--file` overrides the path. Exit codes: 0 for success and for `status` when maintenance is active; 1 for `status` when it is not; 2 when the flag cannot be written or removed (message says only root can, on purpose); argparse errors exit 2 as usual. Only `start` and `end` change anything; both need root. Tests: `tests/test_pagerduty_maintenance.py`.

- **Auto-Resolution:** Once a previously-failing check starts passing again, `auto_resolve()` [@ANCHOR: auto_resolve] calls back into Odoo to close out any open incident for that source, and is written to swallow (not raise) any RPC failure so a resolve call that can't reach Odoo doesn't crash the polling thread that made it.
