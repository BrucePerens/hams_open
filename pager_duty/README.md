# 📟 Pager Duty & Generalized Monitoring (`pager_duty`)

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).*

---

### ⚠️ IMPORTANT: JULES VM ENVIRONMENT NOTE
If you are developing or testing this module within the Jules VM environment, please consult [docs/JULES_ISSUES.md](docs/JULES_ISSUES.md) for critical information regarding PostgreSQL socket permissions and Many2many field name sensitivities.

[@ANCHOR: pager_duty_module_root]

The Pager Duty module is an enterprise-grade Site Reliability Engineering (SRE) suite designed to keep your Odoo infrastructure running smoothly. It provides active monitoring, intelligent alerting, and automated incident management.

## 🌟 What It Does

*   **Active System Monitoring:** Continuously checks the health of web workers, background daemons, databases, network connections, and hardware.
*   **Smart Alerting:** Routes alerts to the right person based on Odoo Calendar schedules, preventing alert fatigue.
*   **Automated Escalation:** Escalates unacknowledged incidents to wider groups or management.
*   **Incident Analytics:** Tracks Mean Time to Acknowledge (MTTA) and Mean Time to Resolve (MTTR).
*   **Helpdesk Integration:** Automatically creates tickets in the Helpdesk module for incoming incidents.
*   **Multi-Website Support:** Partition monitoring checks and incidents by website to support multi-tenant Odoo deployments. This includes record-level security rules and optimized indices for strict data isolation.

## 🛠️ How to Set It Up

1.  **Dependencies:** Ensure `redis`, `psutil`, `ntplib`, `pymysql`, and `ldap3` are installed in your Python environment.
2.  **Installation:** Install the `pager_duty` module from the Odoo Apps menu.
3.  **Daemon Configuration:**
    *   Navigate to **Pager Duty > Monitoring Checks**.
    *   Use the "JSON Configuration Tools" (Import/Export) to synchronize the database with the daemon's `pager_config.json`.
    *   Deploy and start the Python daemons located in the `daemon/` directory (see `DEPLOYMENT.md` for systemd service examples).

## 🚀 Key Features and Operations

### Monitoring Checks
Create diverse checks for your infrastructure:

- **HTTP/HTTPS/HTTP3:** Verify website availability and content.
- **PostgreSQL/MySQL:** Ensure database connectivity and performance.
- **System Resources:** Monitor CPU, RAM, and Disk space.
- **Service Status:** Check systemd services and Docker containers.
- **Hard Drive Health:** Proactive SMART monitoring.
- **Custom Scripts:** Execute sandboxed Bash or Playwright scripts for synthetic journeys.

### Incident Management
- **Dashboard:** The NOC Dashboard provides a real-time overview of active and resolved incidents. It is powered by a high-performance PostgreSQL procedure to minimize latency and includes burn-in protection for long-term display.
- **Acknowledgement:** Engineers can acknowledge incidents to stop further escalation.
- **Auto-Resolution:** The system automatically resolves incidents when the underlying check returns to a healthy state.
- **Escalation:** Unacknowledged incidents are automatically escalated after 15 minutes to ensure attention.

### On-Call Scheduling
Integrates with the Odoo Calendar. Mark calendar events as "Pager Duty Shift" to define the current on-call engineer.

---

# Technical Documentation

<system_role>
**Context:** Technical documentation strictly for Software Engineers, SREs, and Integrators.
</system_role>

## 1. Architecture Overview (CQRS)
The module follows a **Command Query Responsibility Segregation (CQRS)** pattern. Odoo serves as the configuration and reporting plane, while standalone Python daemons handle the high-frequency execution loops.

### Key Components:
*   **Control Plane:** Odoo records (`pager.check`) define what to monitor. [@ANCHOR: generalized_pager_config]
*   **Data Plane (Daemons):**
    *   `generalized_monitor.py`: Executes standard checks (HTTP, TCP, SQL, etc.) via a micro-privilege service account. It is designed to "fail fast" if system dependencies are missing. [@ANCHOR: daemon_execute_check]

    *   `pager_log_analyzer.py`: Tails system logs for regex matches in real-time. It runs chrooted to `/var/log`, drops all kernel capabilities, and de-escalates to `nobody:adm`. [@ANCHOR: COMM_pd_log_api_i18n]
    *   `pager_smart_spooler.py`: Securely collects hardware health data (SMART).
    *   `pager_synthetic_spooler.py`: Executes sandboxed (Bubblewrap) Playwright/Bash tests. [@ANCHOR: synthetic_i18n]
*   **Inter-Process Communication (IPC):** Uses Redis Pub/Sub and Queues for high-speed communication between Odoo workers and background daemons.

### Security & Micro-Privileges:
*   **Zero-Sudo RPC:** Daemons authenticate via the `pager_service_internal` service account. No `sudo()` is used. All operations utilize `with_user()` for minimum privilege execution. High-privilege RPCs are protected by allow-lists. [@ANCHOR: rpc_ensure_executable_security]

*   **Config Isolation:** The location of the daemon configuration file is managed through system parameters, isolated by service accounts. [@ANCHOR: generalized_pager_config_path]
*   **Sandboxing:** Synthetic checks run inside a strict **Bubblewrap (bwrap)** sandbox with optional network isolation.
*   **Service Accounts:** The module uses `zero_sudo.security.utils` to securely escalate privileges within Odoo's ACL framework.
*   **Multi-Website Isolation:** Data is partitioned by `website_id`. The NOC Dashboard, incident reporting, and on-duty scheduling all respect `website_id` for strict multi-tenant isolation.

---

## 2. Developer API & Integration

This section documents all public functions, models, and controllers in the `pager_duty` module and how developers can utilize them.

### `pager.check` (Models)
The core model defining monitoring checks.
*   `rpc_ensure_executable(self, cmd_name)`: Validates if a daemon command is allowed to execute based on strict security allow-lists. Developers should call this when adding new bash-level integrations.
*   `check_heartbeat_rpc(self, hb_uuid, interval_sec)`: Registers a heartbeat from an external daemon to confirm it is actively polling.
*   `action_pull_from_json(self)`: Syncs checks from `pager_config.json` into the database. Often mapped to a UI button.
*   `action_push_to_json(self)`: Exports database checks to `pager_config.json` for daemon consumption.
*   `action_autodiscover(self)`: Scans the system to automatically generate recommended monitoring checks for web services, databases, etc.
*   `action_trigger_check(self)`: Manually forces an immediate execution of the monitoring check.
*   `update_lets_encrypt_domains(self, domains)`: Automatically updates SSL monitoring based on discovered Let's Encrypt certificates.

### On-Call Scheduling (`calendar.event` extension)
*   `get_current_on_duty_admin(self)`: Retrieves the `res.users` record of the currently active responder based on calendar shifts.
    ```python
    on_duty_user = self.env["calendar.event"].get_current_on_duty_admin()
    ```
    [@ANCHOR: test_pager_notification]

### `pager.incident` (Models)
Handles the lifecycle of monitoring alerts.
*   `report_incident(self, vals)`: Programmatically reports a new incident. Automatically handles deduplication and rate-limiting.
    ```python
    self.env["pager.incident"].report_incident({
        "source": "Custom Script",
        "severity": "high",
        "description": "Critical failure detected"
    })
    ```
    [@ANCHOR: report_incident_rate_limit]
*   `action_escalate_unacknowledged(self)`: Checks all unacknowledged incidents and escalates them to administrators if the 15-minute SLA is breached. Automatically invoked by cron.
*   `auto_resolve_incidents(self, source, website_id=None)`: Resolves any active incidents matching the provided source. Call this when a system returns to a healthy state.
*   `action_acknowledge(self)`: Acknowledges the current incident, halting its escalation timer.
*   `get_board_data(self)`: Generates real-time, aggregated JSON metrics for the NOC Dashboard UI.

### `pager.incident.ticket.adapter` (Models)
*   `action_generate_helpdesk_ticket(self)`: Converts an existing Pager Duty incident into a Helpdesk ticket, assigning the active on-call responder. Used in UI buttons or automated flows.
    [@ANCHOR: COMM_pd_helpdesk_adapter]

### `pager.log.analyzer` (Models)
*   `rpc_update_state(self, uuid, state, result_payload)`: Updates the async task status of a real-time log search query (called by `pager_log_analyzer.py`).

### Controllers
Provides HTTP endpoints for daemon-to-Odoo communication.
*   `pager_board(**kw)`: Serves the NOC Dashboard HTML.
*   `update_domains(domains=None, api_identity=None, **kwargs)`: Receives domain updates for automated SSL tracking.
*   `search_logs(file_path, regex_query)`: Initiates a background log search task.
*   `search_logs_poll(job_id)`: Long-polls for the result of an active log search task.
*   `get_log_files()`: Retrieves a list of parseable log files from the server.
*   `ping(**kw)`: Returns a simple 200 OK for basic Odoo connectivity checks.
*   `heartbeat(hb_uuid, **kw)`: HTTP endpoint for daemons to transmit their heartbeats without XML-RPC.

---

## 3. Extending the System
To add a new monitoring plugin:
1.  **Model:** Add the type to `check_type` in `pager_check.py`.
2.  **View:** Update `pager_check_views.xml` with relevant fields (visible only for the new type).
3.  **Daemon:** Implement the logic in `execute_check()` within `generalized_monitor.py`.
4.  **Test:** Add an isolated test case in `test_generalized_monitor.py`.

---

<stories_and_journeys>
## 4. Architectural Stories & Journeys

*   [Story: Scaling the Watchtower](docs/stories/automated_monitoring_setup.md)
*   [Story: Finding the Needle in the Haystack](docs/stories/log_anomaly_detection.md)
*   [Story: The Midnight Guardian](docs/stories/on_call_alerting.md)
*   [Story: The Data-Driven Post-Mortem](docs/stories/performance_analytics.md)
*   [Journey: Daemon Execution Loop](docs/journeys/daemon_execution_loop.md)
*   [Journey: Escalation Pathway](docs/journeys/escalation_pathway.md)
*   [Journey: Incident Lifecycle](docs/journeys/incident_lifecycle.md)
*   [Journey: Synthetic Monitoring Flow](docs/journeys/synthetic_monitoring_flow.md)
</stories_and_journeys>

---

## 5. Testing & Maintenance
Run module tests using the unified test runner:
```bash
python3 tools/test.py -u pager_duty --already-provisioned
```
Daemon tests are located in `pager_duty/daemon/` and run via pure `unittest`. **Do not import Odoo packages in daemon tests.**
