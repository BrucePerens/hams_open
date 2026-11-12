# PagerDuty Monitor Daemon

This directory contains the Generalized Monitor Daemon for infrastructure incident detection and reporting.

### Functions
- **Service Monitoring**: Polls multiple protocols and services (HTTP/3, TCP, UDP, SSL, PostgreSQL, Redis, RabbitMQ, DNS, SMTP, IMAP, etc.) for availability and expected payload responses.
- **System Metrics**: Checks system disk, memory, CPU, and I/O load.
- **Auto-Provisioning**: Interacts with Odoo to provision missing binaries (`rpc_ensure_executable`) dynamically if dependencies are absent. It tries 12 times, 10 seconds apart; if a binary still cannot be provisioned, the daemon reports a critical "Daemon Boot" incident and exits.
- **Incident Reporting**: Reports incidents securely to Odoo via JSON-2 API or uses an automated SMTP fallback if the RPC connection fails. The JSON-2 API is Odoo's `/json/2/<model>/<method>` HTTP endpoint; the daemon sends the value of `ODOO_PASSWORD` (or, when that is unset, `ODOO_RPC_KEY`, the name `daemon_key_manager` writes into a daemon's key file) as a bearer token, so the Odoo account it acts as is the owner of that key (intended to be `pager_service_internal`; `ODOO_USER` / `ODOO_RPC_LOGIN` is read but not sent), and it refuses to start when neither is set. The SMTP fallback needs the `PAGER_FALLBACK_EMAIL` and `SMTP_HOST` environment variables; without them a failed report is only written to the log. If `PAGER_WEBHOOK_URL` is set, every report is also posted to that webhook. During a planned restart the SMTP fallback stays quiet while the pagerduty maintenance flag is active (`sudo pagerduty-maintenance start`; see "Pagerduty maintenance" in `../README.md`); the flag expires by itself and is capped at 2 hours.
- **Auto-Resolution**: After a failing check passes three times in a row, the daemon asks Odoo to resolve that check's open incidents.

The daemon reads its configuration file once, at startup, so restart it after pushing new checks from Odoo.

### File Structure

- `pagerduty_maintenance.py`: the `pagerduty-maintenance` command (`start`, `end`, `status`) and the flag reader the daemon imports. Standard library only; install it as `/usr/local/sbin/pagerduty-maintenance`.
- `generalized_monitor.py`: The core monitoring script executing continuous checks.
- `pager_log_analyzer.py`: Tails the configured log files for the configured regex patterns and answers interactive log searches. It starts as root, chroots to `/var/log`, drops its capabilities and switches to `nobody:adm`. It talks only to Redis; `generalized_monitor.py` forwards its findings to Odoo.
- `pager_smart_spooler.py`: Run as root every 10 minutes by `pager-smart-spooler.timer`; writes `smartctl` health results to `/var/log/pager_smart_spool.json` for the `smart` check type.
- `pager_synthetic_spooler.py`: Runs as root and executes the Playwright, Sandboxed Bash and Sandboxed Arbitrary Executable checks inside a Bubblewrap (`bwrap`) sandbox, writing results to `/var/log/pager_synthetic_spool.json`.
- `pager_mcp_server.py`: A Model Context Protocol (MCP) server for AI triage, offering only `list_incidents`, `get_incident` and `add_incident_note`. It authenticates with an API key (`PAGER_MCP_API_KEY`) issued to the narrowly scoped `user_pager_mcp_triage_service` account and cannot acknowledge or resolve incidents.
- `check_github_pat_expiry.py`: A stand-alone script for a "Synthetic Journey (Script)" check that warns before an API token expires, raising the severity as the expiry date approaches.
- `check_provisioned_units.py`: A stand-alone script for a "Synthetic Journey (Script)" check that warns when something we built is not in operation: for every unit file the hams_shared MANIFEST installs it asks systemd whether the unit is installed, and whether its timer, path or long-running service is active (an opt-in timer that is only linked is left alone). `high` severity, so it pages.
- `check_auth_cert_expiry.py` (`--profile turn` for the TURN server's certificate): A stand-alone script for a "Synthetic Journey (Script)" check on the auth.hams.com server certificate: reads `/etc/hams/auth/auth.crt` with `openssl x509 -enddate` (or asks the live server when the file is not readable), warns under 30 days (`low`) and pages under 14 (`high`, the lowest severity that pages).
- `check_recordings_disk_usage.py`: A stand-alone script for a "Synthetic Journey (Script)" check on the filesystem that holds a directory of recordings (default `/var/lib/hams/qso_recordings`). The built-in disk check has only a failure threshold, so this one warns (`medium`, recorded, not paged) from 70 percent used and fails (`high`, pages) from 85 percent (`--warn`, `--fail`), and prints a volume estimate (files, bytes, bytes per day over 7 days, days to the failure line) that a person can also get by running it by hand. It lists file names and sizes only and never opens a recording.
- `*.service`, `*.timer`: systemd units for the daemons above (see `../DEPLOYMENT.md`).

### Rotated key (`OdooClient`)

The monitor reads `ODOO_RPC_KEY` from its systemd `EnvironmentFile` (`/opt/hams/etc/keys/pager_duty.env`, overridable with `PAGER_KEY_ENV_FILE`) when it starts. A daemon key
rotation rewrites that file but cannot restart a running daemon (Odoo has no privilege to), so the client re-reads it itself.

### `[@ANCHOR: pager_duty:odoo_client_reload_key]` `OdooClient.reload_key()`

**Purpose.** Picks up a rotated Odoo API key without a restart. On 2026-10-27 every daemon key on hams1 was rotated; the backup worker kept the old key for nine days
(ticket 116) and every other long-running daemon that reads its key once has the same exposure. `OdooClient.execute()` (`[@ANCHOR: pager_duty:odoo_client_execute]`) calls this after
an HTTP 401 and sends the call once more with the new key. Every credentialed request of this directory (`OdooClient.execute()`, the MCP adapter's `OdooClient.execute()` `[@ANCHOR: pager_duty:mcp_odoo_client_execute]` and the GitHub token check `[@ANCHOR: pager_duty:fetch_github_pat_expiry]`) goes through an opener that refuses redirects, because urllib copies the `Authorization` header to the host a 3xx answer names; a 3xx surfaces as an `HTTPError`. Each of the three files has its own `_NoRedirect.redirect_request` (standalone daemons share no module): `[@ANCHOR: pager_duty:no_redirect_monitor]` (`generalized_monitor.py`), `[@ANCHOR: pager_duty:no_redirect_mcp]` (`pager_mcp_server.py`) and `[@ANCHOR: pager_duty:no_redirect_github_pat_expiry]` (`check_github_pat_expiry.py`); each takes the urllib redirect arguments `(req, fp, code, msg, headers, newurl)` and returns `None`, which makes urllib raise the 3xx as an `HTTPError`. Tested by `pager_duty/tests/test_credential_redirect.py`.

**Call interface.**

| Name | Type | Meaning |
|---|---|---|
| (none) | | Reads the module constant `ODOO_KEY_ENV_FILE`, rewrites the client's `Authorization` header. |
| returns | `bool` | `True` only when the file held a non-empty `ODOO_RPC_KEY` that differs from the key in use, which is now in use. `False` when the file is unreadable, has no such line, or holds the same key. |

Errors: none raised; an unreadable file is logged as a warning (the reason only, never the key). A 401 that the reload cannot cure raises as before (`JSON-2 API Error 401`).

**Special concerns.** One blocking read of a small file, at most once per failing call; never logs a key; only the last `ODOO_RPC_KEY=` line counts, quotes are removed.

**See also.** `test_odoo_client_reloads_a_rotated_key` in `test_odoo_client_key_reload.py`; `backup_management/daemon/main.py` (`_reload_odoo_key`) and `zero_sudo/daemon/json_rpc_client.py`, which do the same.

### Log analyzer (`pager_log_analyzer.py`)

Tails the files of the active `pager.log.file` rows for the active `pager.log.pattern` regexes (all patterns apply to every file, case-insensitively, one line at a time) and queues each match on the Redis list `pager_log_anomalies`; `generalized_monitor.py` pops that list and reports "Log Analyzer: <pattern name>" incidents to Odoo (dropping them while the pagerduty maintenance flag is active). It also answers the interactive search in Odoo (`log_search_req` channel, results on `pager_log_search_res_queue`).

On hams.com the unit is `pager-log-analyzer.service` in the `hams_shared` MANIFEST: root, `/opt/hams/src/hams_open/pager_duty/daemon`, only `redis.env`, `PAGER_CONFIG_PATH=/opt/hams/spool/pager_duty/pager_config.json`, the monitor's sandbox and the five capabilities of its own isolation sequence. It starts as root, reads the configuration, connects to Redis, chroots to `/var/log`, empties the capability bounding set and becomes `nobody:adm`; only then does it open any log file, so a file must be readable by group `adm` (check with `sudo -u nobody -g adm test -r <file>`). Like the monitor it reads the configuration once at start: restart it after "Push to Daemon". The sample `pager-log-analyzer.service` next to this file is for hosts laid out like a stock Odoo install.

#### `[@ANCHOR: pager_duty:log_analyzer_resolve_redis_host]` `resolve_redis_host(host)`

**Purpose.** Gives the analyzer an IP address for Redis before it chroots. Inside `/var/log` there is no `/etc/hosts`, `resolv.conf` or `nsswitch.conf`, so the name `redis` (the default `REDIS_HOST`) could never be looked up again when the connection drops and the client reconnects.

**Call interface.**

| Name | Type | Meaning |
|---|---|---|
| `host` | `str` | Host name or address from `REDIS_HOST`. Borrowed, not modified. |
| returns | `str` | The first IPv4 address `socket.getaddrinfo` gives, else the first address of any family (text form); `host` itself when it is already an address, or cannot be resolved. |

Errors: none raised; a resolution failure returns `host` so the following Redis connection attempt reports the real error and the daemon exits for systemd to retry.

**Special concerns.** One blocking name lookup at start (before the chroot, as root). No side effects, no secrets.

**See also.** `test_05_resolve_redis_host_returns_an_address_and_leaves_an_unresolvable_name_alone`; `log_analyzer_main`.

#### `[@ANCHOR: pager_duty:log_analyzer_push_anomaly]` `push_anomaly(r_client, payload, retry_seconds=5)`

**Purpose.** Queues one log match for the monitor and refuses to lose it. Before this function, a Redis error in a tailer thread was uncaught: the thread ended silently while the daemon stayed alive and stopped watching its file.

**Call interface.**

| Name | Type | Meaning |
|---|---|---|
| `r_client` | `redis.Redis` | The connection the daemon made in `main()`; shared by all tailer threads (redis-py clients are thread-safe). Borrowed. |
| `payload` | `dict` | The anomaly (see below), JSON-serialisable. Borrowed. |
| `retry_seconds` | `int` or `float` | Pause between attempts after a Redis error, seconds; default 5; 0 is allowed (tests). |
| returns | `None` | Returns only after `LPUSH` succeeded. |

The payload, as `tail_file` builds it and the monitor reads it:

| Key | Type | Meaning |
|---|---|---|
| `source` | `str` | `"Log Analyzer: " + the pattern's name`; becomes the incident source. |
| `severity` | `str` | `low`, `medium`, `high` or `critical`, the pattern's severity. |
| `description` | `str` | The matching log line, whitespace-stripped. |
| `website_id` | `int` or `None` | The pattern's website, `None` for a global pattern. |

Errors: `redis.RedisError` is caught, logged at error level and retried without limit; any other exception propagates (a programming error should be visible).

**Special concerns.** Blocks only the calling tailer thread, for as long as Redis is down; the log file keeps growing and is read on afterwards, so nothing is lost. No lock is taken. The queue is at-least-once: a push that succeeded but whose reply was lost is repeated, and the monitor de-duplicates incidents by source. Not a transmit path.

**See also.** `test_06_push_anomaly_waits_out_a_redis_outage_instead_of_losing_the_match`; `tail_file`; `generalized_monitor.py` (the consumer of `pager_log_anomalies`).

#### `[@ANCHOR: pager_duty:tail_file]` `tail_file(r_client, fp, compiled_patterns)`

**Purpose.** The tailing loop for one log file; `main()` runs one per configured file in a thread pool. It follows the file like `tail -F`: it starts at the end of the file (history is never re-scanned), follows a rotation to a new file (new inode, read from the start), reads a file again from the start after `copytruncate` (the Odoo logrotate entry keeps the inode and shortens the file), waits while the file is missing, and queues every line that matches any pattern.

**Call interface.**

| Name | Type | Meaning |
|---|---|---|
| `r_client` | `redis.Redis` | Connection for `push_anomaly`. Borrowed. |
| `fp` | `str` | Absolute path as configured (for example `/var/log/postgresql/postgresql-18-main.log`); `translate_path` turns it into the chroot-relative path. |
| `compiled_patterns` | `list[dict]` | One dict per pattern: `name` (str), `severity` (str), `website_id` (int or None), `c_reg` (compiled, case-insensitive regex, tested with `findall` on each line). Borrowed, read only. |
| returns | never | An endless loop; stop it by ending the process. |

Errors: `OSError` while stat-ing, opening or reading is logged and retried after 5 seconds. A missing file is logged once at warning level (again after it was present and disappears) and polled every 5 seconds. A Redis error blocks in `push_anomaly` rather than ending the loop.

**Special concerns.** Runs in its own thread and blocks (sleeps 0.5 seconds when idle, 5 seconds when the file is missing). It opens the file as `nobody:adm` after the chroot, so a file that group `adm` cannot read is logged as `Failed to open` and never watched. One line is examined at a time, so a very long line is read whole. After copytruncate, lines written between the truncation and the next poll are read from the start; lines the writer overwrote before the poll are lost (inherent to copytruncate). Every matching line produces one queue entry; there is no de-duplication here.

**See also.** `test_02_tail_file_reports_a_matching_line_and_ignores_others`, `test_07_tail_file_reads_a_log_again_after_copytruncate_rotation`; `push_anomaly`; `translate_path`.

#### `[@ANCHOR: pager_duty:log_analyzer_main]` `main()`

**Purpose.** The daemon entry point (only run as a script, never on import). Order: resolve the configuration file (`log_analyzer_resolve_config_path`), read the `log_analyzer.files` and `log_analyzer.patterns` lists (a missing or invalid file exits 1; an empty pattern list gets the single default "Kernel Filesystem Corruption" pattern), resolve `REDIS_HOST` to an address and connect (`REDIS_PORT`, `REDIS_USERNAME`, `REDIS_PASSWORD`; failure exits 1), and, when running as root, chroot to `/var/log`, empty the capability bounding set and become `nobody:adm`. Then compile the regexes (an invalid one is logged and skipped), start one `tail_file` thread per file and run the interactive search listener in the main thread.

**Call interface.** No arguments, returns nothing; it does not return normally.

| Environment | Meaning |
|---|---|
| `PAGER_CONFIG_PATH`, `PAGER_CONFIG_DIR` | Which configuration file (see `log_analyzer_resolve_config_path`). |
| `REDIS_HOST` (default `redis`), `REDIS_PORT` (6379), `REDIS_USERNAME`, `REDIS_PASSWORD` | The Redis login; the host is resolved before the chroot. |

Exit codes: 0 when no log file is configured; 1 for a missing or unparsable configuration, a failed Redis connection, a missing `/var/log`, a failed change to `nobody:adm`, or the search listener ending or failing (the process then ends with `os._exit(1)`, because the tailer threads never end and would keep a half-dead daemon alive; systemd restarts it after 10 seconds).

**Special concerns.** Not importable for side effects. Reads the configuration once: restart after "Push to Daemon". After the privilege drop it is `nobody:adm` in a chroot with no capabilities: files can be opened only if group `adm` can read them, and no name resolution works. A capability that cannot be dropped is logged as a warning (the privilege change that follows removes them anyway); failing to change to `nobody:adm` is fatal (exit 1), because the daemon would otherwise go on reading logs as root. It writes nothing to the filesystem. A planned restart of the services is silenced by the pagerduty maintenance flag in the monitor, not here (the chroot cannot read the flag).

**See also.** `test_04_main_exits_cleanly_when_no_files_are_configured`, `test_08_main_exits_the_process_when_the_search_listener_fails`; `tail_file`; `push_anomaly`.

### Running it unattended on a host

- **Dry run:** `python3 generalized_monitor.py --check-once` runs every check in the configuration once, prints `PASS` or `FAIL` and the message for each, reports nothing to Odoo, creates no incident, sends no e-mail and writes no status, and exits 0 only when every check passed. Run it under the unit's own user, environment files and sandbox (for example with `systemd-run`) before enabling the unit.
- **Configuration file:** `PAGER_CONFIG_PATH` names the exported check list explicitly; when it is set and the file is missing the daemon halts and says so instead of falling back to the sample list shipped next to it. Odoo writes the file where the `pager_duty.config_dir` system parameter points ("Push to Daemon"), so that directory must be one the Odoo server may write.
- **Credentials of the PostgreSQL and Redis checks:** a `redis` check with no password of its own logs in with `REDIS_USERNAME` / `REDIS_PASSWORD` from the environment (the login the daemon's own Redis connections use). A `postgres` check with no password of its own logs in with `DB_USER` / `DB_PASS` (and `DB_NAME`) from the environment, which the unit supplies from a root-readable environment file; no password is stored in the database or printed. A check that has its own password uses it unchanged.
- **Consecutive failures and grace:** the `threshold` key of a check (the "Failures Before Reporting" field, default 3 for a new row, 1 for a configuration file that lacks the key) and `grace` (seconds after daemon start during which a confirmed failure is only logged) decide when a failure is reported. The second run of every check follows the first within a minute at most, whatever its interval.
- **hams.com:** the unit is `pager-os-monitor.service` in the `hams_shared` provisioning MANIFEST (`/opt/hams/src/hams_open/pager_duty/daemon`, user `odoo`, config under `/opt/hams/spool/pager_duty`); the sample `pager-os-monitor.service` next to this file is for hosts laid out like a stock Odoo install.
- **The RabbitMQ check is a full AMQP connection:** it authenticates, opens the vhost (`vhost` on the check, else `RMQ_VHOST`, else `/`) and a channel, then closes the channel and the connection with their own handshakes, so the broker sees an ordinary short client instead of an abruptly dropped socket. A check with no credentials of its own logs in with `RMQ_USER` / `RMQ_PASS` from the environment (the unit must load `rabbitmq.env`); there is no `guest` fallback, and with no credentials the check fails with a message saying so. The result text names the failure: refused, timed out, authentication failed, access denied (vhost), protocol error, or lost connection. The credentials are never printed.

## Monitor helper interface (`generalized_monitor.py`)

Each entry below is one function of `generalized_monitor.py`. The first group are pure helpers; the freshness group backs the `freshness` check type (see "Freshness" in `../README.md`).

### `severity_for_days_left(days_left)` [@ANCHOR: pager_duty:severity_for_days_left]

Turns the days remaining on an expiring credential into a severity, so every expiry check escalates the same way. It walks `SEVERITY_THRESHOLDS_DAYS`, which is `(2, critical)`, `(7, high)`, `(14, medium)`, `(30, low)`, from least to most urgent, and returns the severity of the first threshold that `days_left <= threshold` satisfies.

| Name | Type | Meaning |
|---|---|---|
| `days_left` | number | Days until expiry; negative or zero means already expired (gives `critical`). |
| returns | `str` or `None` | `low`, `medium`, `high` or `critical`; `None` when more than 30 days remain (healthy). |

No errors, no side effects. Used by the synthetic expiry scripts such as `check_github_pat_expiry.py`. Test: `tests/test_check_github_pat_expiry.py`.

### `extract_severity_prefix(msg, default="high")` [@ANCHOR: pager_duty:extract_severity_prefix]

The reading half of graduated severity for the `synthetic` check type. A script prints `SEVERITY:<level>` on stderr, `execute_check` turns that into a message starting `[SEVERITY:low|medium|high|critical] `, and this function splits it off again before the incident is reported.

| Name | Type | Meaning |
|---|---|---|
| `msg` | `str` | The check's failure message. |
| `default` | `str` | Severity to use when there is no prefix; `high`. |
| returns | `(str, str)` | `(severity, message without the prefix)`, or `(default, msg)` unchanged when the message does not start with a valid prefix. |

The pattern is anchored at both ends and allows a multi-line remainder. Never raises; pure function. Tests: `test_22_extract_severity_prefix`, `test_22b_extract_severity_prefix_ends_at_the_end_of_the_text`.

### `_human_age(seconds)` [@ANCHOR: pager_duty:human_age]

Formats an age for a message. `seconds` is a number (negative is treated as 0). Returns `"<n>s"` under 120 seconds, `"<n>min"` under two hours, `"<h>h<mm>"` (minutes zero padded) under two days, otherwise `"<n>d"` in whole days. Used in the freshness messages (`newest record is 3h05 old (limit 6h00)`). Pure function, never raises. Test: `ConfigurationTests.test_human_age`.

### `parse_freshness_time(value)` [@ANCHOR: pager_duty:parse_freshness_time]

Reads a time stamp written by a feed, whatever form it is in. Accepts a number, a numeric string, `bytes` (decoded as UTF-8), or an ISO 8601 or `YYYY-MM-DD HH:MM:SS` string (a trailing `Z` is accepted; a time with no zone is taken as UTC).

| Name | Type | Meaning |
|---|---|---|
| `value` | number, `str` or `bytes` | The stored value. Borrowed. |
| returns | `float` or `None` | Epoch seconds; `None` for anything unreadable, including `nan` and `inf`. |

Never raises and has no side effects. It does not range check: a far future time is returned as given and the caller treats a negative age as an error. Tests: `RedisKeyAndFileTests.test_epoch_and_iso_values_are_read`, `RedisKeyAndFileTests.test_iso_time_age_is_computed_in_utc`, `RedisKeyAndFileTests.test_a_missing_key_or_garbage_fails`.

### `parse_systemd_timestamp(text)` [@ANCHOR: pager_duty:parse_systemd_timestamp]

Reads the time form `systemctl show` prints, such as `Tue 2026-10-20 05:17:00 UTC`. The weekday is ignored. `UTC`, `GMT` and `Z` (or no zone word) mean UTC; any other zone name is read as the host's own zone with `time.mktime`, which is what systemd printed.

| Name | Type | Meaning |
|---|---|---|
| `text` | `str` or `None` | The property value. |
| returns | `float` or `None` | Epoch seconds. `None` for empty, `n/a` (the event has not happened), fewer than three words, or text that does not match the format. |

Never raises; pure apart from reading the host zone. Test: `UnitTests.test_systemd_timestamps`.

### `_freshness_one_unit(unit, max_age, now)` [@ANCHOR: pager_duty:freshness_one_unit]

Judges one timer-driven `Type=oneshot` systemd unit for the `unit:` freshness scheme. It finds `systemctl` with `ensure_executable`, runs `systemctl show <unit>` for six properties (10-second timeout) and decides.

| Name | Type | Meaning |
|---|---|---|
| `unit` | `str` | Unit name, for example `backup.service`. |
| `max_age` | number | Seconds the last successful run may be old (and how long a run may take). |
| `now` | number | Epoch seconds, passed in so tests can fix the time. |
| returns | `(bool, str)` | Pass or fail and a message. |

It fails for: a unit that is not loaded, a unit whose `Type` is not `oneshot` (the message points to a data source scheme for daemons), a run in progress for longer than `max_age` ("looks hung"), a last run whose `Result` is not `success`, a last success older than `max_age`, and a unit that has not run since boot when the host has been up longer than `max_age`. A run in progress shorter than `max_age`, a recent success, or no run yet on a recently booted host (`/proc/uptime`) pass. A missing `systemctl` returns `(False, "Missing dependency ...")`. Blocks up to 10 seconds. Tests: `UnitTests.test_a_recent_successful_run_passes`, `UnitTests.test_a_run_older_than_the_limit_fails`, `UnitTests.test_a_failed_last_run_fails`, `UnitTests.test_an_unknown_unit_fails`, `UnitTests.test_a_run_in_progress_passes_until_it_has_run_longer_than_the_limit`, `UnitTests.test_a_long_running_daemon_unit_is_refused_with_advice`.

### `_freshness_unit(units, max_age, now, failing=None)` [@ANCHOR: pager_duty:freshness_unit]

Applies `_freshness_one_unit` to one unit or to several names separated by commas that share one limit. `units` is the text after `unit:`; blanks are skipped; an empty list fails with a message asking for names. With one unit it returns that unit's result. With several it returns `(True, "OK (all N units ran successfully within <age>)")` or `(False, "K of N units are stale: <message>; ...")` naming every bad unit at once, so one incident lists them all.

| Name | Type | Meaning |
|---|---|---|
| `max_age`, `now` | number | As for `_freshness_one_unit`. |
| `failing` | `list` or `None` | When given, each failing unit name is appended; the caller passes the result to `note_failing_units`. |

Sequential: it blocks up to 10 seconds per unit. No exceptions escape for normal failures. Tests: `UnitTests.test_several_units_report_every_stale_one_in_a_single_message`, `UnitTests.test_the_stale_units_are_sent_with_the_incident`.

### `note_failing_units(check, units)` [@ANCHOR: pager_duty:note_failing_units]

Records which systemd units caused this run's failure so `polling_thread` can send them with the incident and the server can open one ticket per unit. `check` is the check dict (its key is `check_key(check)`, the record id or the name); `units` is an iterable of unit names. It stores `CHECK_UNITS[key]` as a list with empty names dropped, order kept and duplicates removed. Returns nothing. The dict is written and read only by that check's own polling thread (which clears it at the start of each pass), so it takes no lock. Test: `UnitTests.test_the_stale_units_are_sent_with_the_incident`.

## Helpers of the monitor and its check scripts

### `_utcnow()` [@ANCHOR: pager_duty:utcnow]

Returns the current UTC time as a naive `datetime` (no `tzinfo`), the form Odoo stores and compares; it replaces `datetime.utcnow()`, deprecated since Python 3.12. No arguments, never raises. Used for `last_run` in `record_check_status` and for the maintenance window test in `is_in_maintenance`. Test: `ConfigurationTests.test_utcnow_is_a_naive_utc_datetime`.

### `_freshness_redis()` [@ANCHOR: pager_duty:freshness_redis]

Builds the Redis client the freshness checks use. Takes no arguments; reads `REDIS_HOST` (default `redis`), `REDIS_PORT` (default 6379), and the module constants `REDIS_USERNAME` and `REDIS_PASS` (from `REDIS_USERNAME` and `REDIS_PASSWORD`; both may be unset on a test box). Returns a `redis.Redis` with 5-second connect and read timeouts. It makes no connection until first used, so it does not raise for an unreachable server; the check that uses it reports the error. The password is never logged. Tests: `ConfigurationTests.test_freshness_redis_connects_to_the_configured_host_and_port`, `ConfigurationTests.test_freshness_redis_defaults_to_the_redis_host`.

### `_failure_detail(exe, unit)` [@ANCHOR: pager_duty:failure_detail]

Explains why a unit is in the failed state, for the failed-units check message. It runs `<exe> show <unit> -p Result -p StateChangeTimestamp` (10-second timeout). `exe` is the `systemctl` path; `unit` is the unit name. Returns text such as `exit-code since 2026-10-18 18:24:17 UTC` (the result, then `since` and the time with the weekday dropped), either part omitted when empty, and an empty string when the command cannot run, times out or fails. Never raises. Test: `FailedUnitsCheckTests.test_failure_detail_is_the_result_and_the_time_without_the_weekday`.

### `amqp_check(host, port, user, password, vhost="/")` [@ANCHOR: pager_duty:amqp_check]

The RabbitMQ check: a real AMQP client session, so the broker logs an ordinary short connection. It opens a `pika.BlockingConnection` (plain credentials, one attempt, 3-second socket timeout, 5-second handshake timeout, 2-second heartbeat), opens and closes a channel, and closes the connection.

| Name | Type | Meaning |
|---|---|---|
| `host`, `port` | `str`, `int` | Broker address. |
| `user`, `password` | `str` | Login. With either empty the check fails at once, saying to set `RMQ_USER` and `RMQ_PASS`; there is no guest fallback. |
| `vhost` | `str` | Virtual host to open, default `/`. |
| returns | `(bool, str)` | `(True, "OK")` or `(False, text)` naming the failure: authentication failed, access denied (vhost), protocol error, timed out, connection refused or connection failed. The text never contains the password. |

No exception escapes. A `finally` block closes any connection still open. Blocks up to about the handshake timeout. Tests: `tests/test_amqp_check.py`.

### `_enddate_of(args, pem=None)` in `check_auth_cert_expiry.py` [@ANCHOR: pager_duty:auth_cert_enddate_of]

Asks `openssl x509 -noout -enddate` for a certificate's expiry. `args` is the extra argument list (`["-in", path]` for a file, `[]` for PEM text on stdin); `pem` is the PEM text or `None`. It runs openssl with a 15-second timeout and returns the result of `parse_enddate` (a timezone-aware expiry time). Raises `ValueError("openssl could not read the certificate: ...")` (first 80 characters of stderr) when openssl exits non-zero; the script's `main` turns that into a paging failure. Reads a public certificate only. Tests: `TestAuthCertExpiryCheck.test_healthy_certificate_exits_zero`, `TestAuthCertExpiryCheck.test_an_unreadable_certificate_pages`.

### `systemctl_show(unit)` in `check_provisioned_units.py` [@ANCHOR: pager_duty:provisioned_units_systemctl_show]

Runs `systemctl show <unit> -p <SHOW_PROPERTIES>` (`LoadState`, `ActiveState`, `UnitFileState`, `Type`, `ConditionResult` and `ConditionTimestamp`) without a timeout and without raising on a non-zero exit, and returns the `Name=Value` lines as a dict. `unit` is a unit file name. An unreadable or empty answer gives an empty dict, which the caller reads as "not installed". It is the default `show` argument of `units_not_in_operation`, so tests pass a fake. Read only. Tests: `SystemctlShowTests`.

### `units_expected_off()` in `check_provisioned_units.py` [@ANCHOR: pager_duty:units_expected_off]

Returns the set of unit names the check must skip. Arguments: `environ` (mapping, default `os.environ`) and `path` (default the tracked `units_expected_off.txt` next to the script). The set is the comma-separated names in `HAMS_UNITS_EXPECTED_OFF` plus the file's names (one per line, `#` starts a comment, blank lines ignored; a missing file adds nothing). Each line of the file should carry the reason and the condition for removing it. Returns a `set[str]`; an unreadable existing file raises `OSError`. Test: `ExpectedOffTests`.

### `main()` of `check_provisioned_units.py` [@ANCHOR: pager_duty:provisioned_units_main]

Entry point of the "Synthetic Journey (Script)" check that pages when something provisioned is not running. It reads the expected-off set, loads the unit list from the `hams_shared` MANIFEST (`expected_units`), and asks `units_not_in_operation`. All in operation: prints `all N provisioned units are in operation` and returns 0. Otherwise prints `SEVERITY:high` and a short list (four names and `+N`) then the full list with reasons on stderr and returns 1, which pages. A MANIFEST that cannot be read (`OSError` or `KeyError`) also prints `SEVERITY:high` and returns 1, so the check cannot go quiet. Argument `argv` is unused. Runs `systemctl` once per unit. Test: `MainTests`.

### `nearest_existing_directory(path)` in `check_recordings_disk_usage.py` [@ANCHOR: pager_duty:recordings_disk_nearest_directory]

Finds a directory that can be measured for a path that may not exist yet. `path` is made absolute; the function climbs to the parent until `os.path.isdir` is true and returns that directory (the filesystem root is the stop). Returns `str`. Never raises, makes only `stat` calls. `filesystem_usage` uses it so a missing recordings directory is measured through its nearest parent. Test: `TestRecordingsDiskUsage.test_a_missing_directory_is_measured_through_its_nearest_parent`.

### `human(nbytes)` in `check_recordings_disk_usage.py` [@ANCHOR: pager_duty:recordings_disk_human]

Formats a byte count with decimal units for a message. `nbytes` is a number (bytes). It divides by 1000 until the value is under 1000 or the unit is TB and returns `999 B` (no decimals for bytes), `1.5 KB`, `2.0 GB`, up to `TB` with one decimal. A negative value prints as bytes. Pure function, never raises. Test: `test_human_sizes_use_decimal_units`.

### `summary_line(path, percent, volume, rate, days_to_fail, fail=85)` in `check_recordings_disk_usage.py` [@ANCHOR: pager_duty:recordings_disk_summary_line]

Builds the one-line recordings report that the check prints. `path` is the directory; `percent` the filesystem percent used; `volume` the tuple from `directory_volume` (file count, total bytes, recent bytes, complete flag, oldest mtime); `rate` bytes per day; `days_to_fail` the estimate or `None`; `fail` the failure percent. The tail reads `no growth seen` (`None`), `already past N%` (zero or less), `more than 10 years to N%` (over 3650 days) or `about N days to N%`; `(listing stopped early, partial)` is added when the scan hit its time budget. Returns `str`. Pure function, never raises for well-formed input. Tests: `test_summary_line_carries_files_size_rate_and_days`, `test_a_huge_horizon_reads_as_more_than_ten_years`.
