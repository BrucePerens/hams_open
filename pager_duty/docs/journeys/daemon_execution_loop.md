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
