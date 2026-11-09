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
an HTTP 401 and sends the call once more with the new key. Every credentialed request of this directory (`OdooClient.execute()`, the MCP adapter's `OdooClient.execute()` `[@ANCHOR: pager_duty:mcp_odoo_client_execute]` and the GitHub token check `[@ANCHOR: pager_duty:fetch_github_pat_expiry]`) goes through an opener that refuses redirects, because urllib copies the `Authorization` header to the host a 3xx answer names; a 3xx surfaces as an `HTTPError`. Tested by `pager_duty/tests/test_credential_redirect.py`.

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
