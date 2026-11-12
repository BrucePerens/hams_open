# Story: Finding the Needle in the Haystack

## Persona
**Dave**, a DevOps Engineer investigating a intermittent "502 Bad Gateway" error that only happens during peak traffic.

## Context
Standard HTTP pings show the system is up, but users are complaining about occasional timeouts.

## The Hunt
Dave uses the Pager Duty Log Analyzer to find the root cause.

1.  **Regex Discovery:** Dave creates a new Log Pattern in Odoo (the editable list is covered by the view test, below) searching for "upstream timed out" in the Nginx logs.

2.  **API Streaming:** The Log Analyzer daemon, running as a privileged background process, begins tailing the logs and streaming matches to the Odoo backend [@ANCHOR: pd_log_api_i18n]. (It talks only to Redis; `generalized_monitor.py` forwards its findings to Odoo, and it drops its privileges after start-up, see `../../daemon/README.md`.)

3.  **Correlation:** Dave sees a spike in these errors every time the "Synthetic Journey Spooler" [@ANCHOR: synthetic_i18n] runs a heavy check against the payment gateway.
4.  **Optimization:** Dave adjusts the jitter and timeout settings in the synthetic check configuration to reduce the load on the Nginx upstream during peak hours.

## Result
The intermittent 502s disappear, and Dave has successfully used the integrated log forensics to solve a complex performance bottleneck.

## Under the Hood: How the Analyzer Actually Tails a Chrooted Log

Dave later gets curious about how the privileged log-tailing daemon actually works, since it runs `chroot`ed into `/var/log` for safety.

1.  **Path Translation:** Because the daemon's own filesystem view is chrooted, a path Odoo stores as `/var/log/nginx/error.log` has to be translated into the chroot-relative path the daemon actually opens -- `translate_path()` [@ANCHOR: translate_path] strips the chroot prefix so the daemon and Odoo can agree on the same logical path without either side needing to know about the other's filesystem layout.

2.  **Tailing:** `tail_file()` [@ANCHOR: tail_file] is the actual polling loop -- it seeks to the end of the file on first open (so it never re-scans old history) and then only examines genuinely new content appended after that point, checking each new line against the configured pattern and pushing a match to Redis the moment one appears.

3.  **On-Demand Search:** When Dave wants to search historical log content rather than wait for a live match, his request is published to Redis and picked up by `redis_search_listener()` [@ANCHOR: redis_search_listener], which runs the search against the real file and pushes the result back for Odoo to pick up.

4.  **Bootstrap:** All of this -- the config load, the real Redis connection, and the real `chroot`+privilege-drop to an unprivileged user -- happens inside the daemon's `main()` entrypoint [@ANCHOR: log_analyzer_main], which only runs when the script is actually executed as the daemon (not on a bare `import`, which is what makes the daemon's own logic safe to unit test).

5.  **Polling From Odoo's Side:** Dave's own Odoo session doesn't talk to Redis directly -- it polls `search_logs_poll` (entry below), a controller endpoint restricted to Pager Duty admins, which checks on the state of his search job.

6.  **Reporting Progress Back:** The daemon reports that job's progress back into Odoo via `PagerLogSearchJob.rpc_update_state` [@ANCHOR: log_search_rpc_update_state], a narrowly-scoped RPC method that only the log-analyzer's own service account is allowed to call, so Dave's poll always reflects the daemon's real, current progress rather than a stale guess.

## Function reference

### Log pattern and log file views [@ANCHOR: test_log_analyzer_views]

Marks the test of the screens Dave uses in step 1: `test_01_log_analyzer_views` in `tests/test_log_analyzer.py` asks Odoo for the list view of `pager.log.pattern` (the architecture must contain the `regex` field) and of `pager.log.file` (it must contain `filepath`). Both lists are in `views/log_analyzer_views.xml`, are editable at the bottom of the list, and show name, website and company (when those groups are active), the regular expression or path, the severity for a pattern and an active toggle. A pattern's regex is compiled case-insensitively by the daemon and applies to every configured file; its severity becomes the incident severity. After editing, "Push to Daemon" exports them and the analyzer is restarted, because it reads the configuration once. The test takes no arguments; a broken view makes `get_view` raise.

### `search_logs_poll(self, job_id)` [@ANCHOR: search_logs_poll]

The JSON-RPC route `POST /api/v1/pager/logs/search_poll` (`controllers/log_api.py`, `auth="user"`) that Dave's browser calls once a second after starting a search. Argument: `job_id` (`str`, the uuid returned by `search_logs`). The caller must be a `group_pager_admin` member, else `AccessError`. It looks up the `pager.log.search.job` with that uuid in the caller's own environment. Returns `{"error": "Job not found"}` for no match; `{"status": "pending"}` while the daemon has not answered; `{"error": <payload or "Daemon processing error">}` when the job state is `error`; otherwise `{"status": "done", "matches": [...]}` from the job's `result_payload` JSON (`matches` empty when absent), or `{"error": "Invalid result payload"}` when that text is not valid JSON. Read only, no locking; it only reads what `PagerLogSearchJob.rpc_update_state` wrote. The page gives up after 30 polls. Tests: `test_02b_search_logs_poll_reports_job_state` and `test_02c_search_logs_poll_rejects_a_non_admin` in `tests/test_controllers.py`.
