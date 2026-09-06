# Journey: Daemon Execution Loop

This journey tracks the flow of data from Odoo configuration to the standalone monitoring daemon.

## 1. Configuration in Odoo
- **Entry:** Admin navigates to Monitoring Checks [@ANCHOR: test_pager_view].
- **Schema:** Data is stored in the `pager.check` model.

## 2. Sync to Daemon
- **Wizard:** The admin uses the JSON configuration tools in Monitoring Checks [@ANCHOR: generalized_pager_config].
- **Export:** `action_push_to_json()` transforms ORM records into a JSON structure.
- **Persistence:** The file is written to the daemon's local filesystem (e.g., `pager_duty/daemon/pager_config.json`).

## 3. Execution Cycle
- **Boot:** `generalized_monitor.py` starts and parses the JSON.
- **Dependency Check:** It verifies required system binaries [@ANCHOR: daemon_verify_dependencies].

- **Watchdog:** The main thread starts an execution thread for each check and monitors their heartbeats [@ANCHOR: daemon_main_loop].

- **Isolation:** Each check type (HTTP, XML-RPC, Heartbeat) runs in its own isolated logic block [@ANCHOR: daemon_execute_check].

- **Failover:** If Odoo is unreachable, the daemon falls back to direct `SMTP` or `Webhook` alerts [@ANCHOR: daemon_report_incident].

## 4. Feedback Loop
- **Status Reporting:** Results are pushed back to Odoo via XML-RPC [@ANCHOR: daemon_report_incident].

- **Dashboard:** The NOC Board [@ANCHOR: pager_board_data] reflects the latest check statuses in real-time.

## 5. The RPC Client and Bootstrap

- **Configuration Parsing:** Before anything else runs, `parse_env()` [@ANCHOR: parse_env] reads the daemon's own environment variables (Odoo URL, database name, service credentials) into a plain config object the rest of the daemon consumes.

- **Client Construction:** `get_odoo_client()` [@ANCHOR: get_odoo_client] builds the one `OdooClient` instance every thread shares, falling back to a same-host default (`odoo_db`) when the environment doesn't specify one explicitly.

- **JSON-2 Transport:** `OdooClient.__init__` [@ANCHOR: odoo_client_init] stores the base URL, database, and API key.

- **JSON-2 Transport, continued:** `OdooClient.execute()` [@ANCHOR: odoo_client_execute] is the one method every check thread calls to actually make an RPC -- it POSTs to Odoo's `/json/2/<model>/<method>` endpoint and parses the JSON response, so the rest of the daemon never touches `urllib` directly.

- **Dependency Verification:** Before trusting a check that shells out to a binary (`curl`, `ping`, a synthetic script), the daemon confirms the binary is actually executable on this host [@ANCHOR: ensure_executable], so a missing dependency fails with a clear message rather than a confusing subprocess error deep in a check thread.

## 6. Per-Check Execution Threads

- **Maintenance Windows:** Before running a check's logic, the thread asks whether the check is inside a configured maintenance window [@ANCHOR: is_in_maintenance] -- if so, the thread skips alerting entirely for that cycle, so planned maintenance doesn't page anyone.

- **Polling Thread:** Each polled (non-heartbeat) check runs inside its own long-lived polling thread [@ANCHOR: polling_thread], which loops forever: run the check, record a heartbeat for the watchdog, sleep, repeat.

- **Log Tail Thread:** Log-pattern checks instead run inside a dedicated log-tailing thread [@ANCHOR: log_tail_thread], which follows a file's new appended content the same way `tail -f` would, rather than re-reading the whole file every cycle.

- **SMTP/Webhook Fallback:** If the Odoo RPC itself is unreachable, `fallback_notify()` [@ANCHOR: fallback_notify] sends the alert directly via SMTP or a webhook instead, so a down Odoo instance doesn't also mean a silent monitoring fleet.

- **Auto-Resolution:** Once a previously-failing check starts passing again, `auto_resolve()` [@ANCHOR: auto_resolve] calls back into Odoo to close out any open incident for that source, and is written to swallow (not raise) any RPC failure so a resolve call that can't reach Odoo doesn't crash the polling thread that made it.
