# Story: Finding the Needle in the Haystack

## Persona
**Dave**, a DevOps Engineer investigating a intermittent "502 Bad Gateway" error that only happens during peak traffic.

## Context
Standard HTTP pings show the system is up, but users are complaining about occasional timeouts.

## The Hunt
Dave uses the Pager Duty Log Analyzer to find the root cause.

1.  **Regex Discovery:** Dave creates a new Log Pattern in Odoo [@ANCHOR: test_log_analyzer_views] searching for "upstream timed out" in the Nginx logs.

2.  **API Streaming:** The Log Analyzer daemon, running as a privileged background process, begins tailing the logs and streaming matches to the Odoo backend [@ANCHOR: pd_log_api_i18n].

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

5.  **Polling From Odoo's Side:** Dave's own Odoo session doesn't talk to Redis directly -- it polls `search_logs_poll` [@ANCHOR: search_logs_poll], a controller endpoint restricted to Pager Duty admins, which checks on the state of his search job.

6.  **Reporting Progress Back:** The daemon reports that job's progress back into Odoo via `PagerLogSearchJob.rpc_update_state` [@ANCHOR: log_search_rpc_update_state], a narrowly-scoped RPC method that only the log-analyzer's own service account is allowed to call, so Dave's poll always reflects the daemon's real, current progress rather than a stale guess.
