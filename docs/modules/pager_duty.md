# 📟 Pager Duty & Generalized Monitoring (`pager_duty`)

*Copyright © Bruce Perens K6BP. AGPL-3.0.*

**Context:** Technical documentation strictly for LLMs and Integrators.

---

## 1. Overview & Architecture
The Pager Duty module is an enterprise-grade Site Reliability Engineering (SRE) suite. It uses a **CQRS Architecture**: Odoo acts purely as the configuration control plane (`pager.check`), while a standalone Python daemon (`generalized_monitor.py`) handles the execution loop and state holding outside the WSGI workers.

### Key Architectural Features:
* **Zero-Sudo RPC:** The daemon pushes incidents to Odoo using the `pager_service_internal` micro-account. This triggers automated notifications [@ANCHOR: test_pager_notification] and implements Redis TTLs to prevent alert spam [@ANCHOR: report_incident_rate_limit].
* **Documentation Injection:** The module automatically provisions its documentation payload into the `knowledge.article` API upon installation. [@ANCHOR: doc_inject_pager_duty]
* **Watchdog Threading:** The daemon wraps every check in an isolated thread. The main thread acts as a watchdog, forcibly terminating and restarting the daemon if a thread hangs beyond its timeout threshold.
* **Airgapped SMTP & Webhooks:** If the Odoo XML-RPC interface crashes (500 Error / Connection Refused), the daemon catches the exception and connects directly to the external `SMTP_HOST` or posts to `PAGER_WEBHOOK_URL` to fire the alert.
* **Self-Healing Dependencies:** The daemon gracefully verifies system dependencies (e.g., `docker`, `pg_dump`, `nginx`) via `shutil.which`. For Zero Trust edge integration, if the `cloudflared` binary is missing, it dynamically downloads and executes the static GitHub release without requiring administrative intervention.
* **Stochastic Jitter:** Check loops offset their start times randomly to prevent resource "thundering herds".
* **Intelligent Calendar Routing:** Natively extends Odoo's `calendar.event` model (`is_pager_duty=True`). When an incident fires, the ORM queries the calendar for the active shift and routes the internal message/email precisely to the user currently on call.
* **Cascading Suppression & Maintenance:** The daemon parses `parent` and `maint_start`/`maint_end` dependencies to short-circuit execution natively, preventing alert storms. It also automatically resolves incidents when systems recover. [@ANCHOR: auto_resolve_incidents]
* **Push Monitoring (Heartbeat):** The `/api/v1/pager/heartbeat/<uuid>` REST endpoint accepts check-ins from external bash scripts. The daemon queries `check_heartbeat_rpc` via XML-RPC to verify TTL breaches.
* **Multi-Tier Escalation:** An Odoo `ir.cron` (`cron_escalate_incidents`) sweeps for forgotten incidents and escalates them to the whole admin group. [@ANCHOR: test_pager_escalation]
* **SRE Analytics:** The `pager.incident` model auto-computes `mtta` and `mttr` in minutes during state transitions, such as when an incident is acknowledged. [@ANCHOR: action_acknowledge_incident] Active and resolved incidents are aggregated and presented on the NOC Dashboard [@ANCHOR: pager_board_data], which enforces strict URL authentication checks [@ANCHOR: test_pager_board_url].

---

## 2. Configuration Schema (`pager_config.yaml`)
The daemon parses this YAML file on boot. The `ENV:` prefix securely dynamically injects credentials from the `.env` vault. The Odoo DB checks are exported into this generalized JSON/YAML format for the daemon to consume. [@ANCHOR: generalized_pager_config]

```yaml
checks:
  - name: "WSGI HTTP Ping"
    type: http
    target: [http://127.0.0.1:8069/api/v1/pager/ping](http://127.0.0.1:8069/api/v1/pager/ping)
    expect: '{"status": "ok"}'
    interval: 60
    grace: 120  # Startup grace period suppression
    parent: "Odoo XML-RPC Handshake"
    maint_start: "2026-03-15 00:00:00"
    maint_end: "2026-03-15 02:00:00"
  - name: "Nightly Backup"
    type: heartbeat
    uuid: "123e4567-e89b-12d3-a456-426614174000"
    interval: 86400
  - name: "Odoo XML-RPC Handshake"
    type: xmlrpc
    target: [http://127.0.0.1:8069/xmlrpc/2/common](http://127.0.0.1:8069/xmlrpc/2/common)
    rpc_method: version
    expect: "server_version"
    interval: 60
```

---

## 3. How to Create a New Monitoring Plugin
To extend the daemon with a new capability (e.g., a `docker` API health check), you must execute these 4 steps in unison:

1. **Database Schema (`pager_check.py`):**
   Add your new type to the `check_type` Selection field. Add any new specific parameter fields (e.g., `docker_container_name`).
2. **Configuration Wizard (`pager_config_wizard.py`):**
   Update `action_generate_yaml()` to pull your new field from the DB and write it to the dict. Update `action_save_to_file_and_db()` to parse the field back from the YAML dict into the DB model.
3. **User Interface (`pager_check_views.xml`):**
   Inject your new fields into the notebook pages, using `invisible="check_type != 'your_type'"`. Backend views are tested to ensure rendering stability. [@ANCHOR: test_pager_view]
4. **Daemon Execution (`generalized_monitor.py`):**
   Add an `elif ctype == 'your_type':` block to the `execute_check(check)` function. It MUST return a tuple: `(True, "OK")` on success, or `(False, "Error Message")` on failure. Do not trigger alerts directly within this function.

---

## 4. Programmatic Setup & Hooks
**The Secure Cached Resolver Pattern (ADR-0066)**: The `pager_duty` module offers high-performance `@tools.ormcache` resolvers for cross-module use. ALWAYS use these instead of `.search()` in frontend controllers or background daemons to prevent database exhaustion. Callers **MUST** pass their own `override_svc_uid` to execute the database search under their own service account's context.
* **`pager.check._get_check_id_by_uuid(hb_uuid, override_svc_uid=None)`**: Resolves a heartbeat UUID string to its database ID safely.

---

<stories_and_journeys>
## 5. Architectural Stories & Journeys

For detailed narratives and end-to-end workflows, refer to the following:

### Stories
* [Scaling the Watchtower](docs/stories/pager_duty/automated_monitoring_setup.md)
* [Finding the Needle in the Haystack](docs/stories/pager_duty/log_anomaly_detection.md)
* [The Midnight Guardian](docs/stories/pager_duty/on_call_alerting.md)
* [The Data-Driven Post-Mortem](docs/stories/pager_duty/performance_analytics.md)

### Journeys
* [Daemon Execution Loop](docs/journeys/pager_duty/daemon_execution_loop.md)
* [Escalation Pathway](docs/journeys/pager_duty/escalation_pathway.md)
* [Incident Lifecycle](docs/journeys/pager_duty/incident_lifecycle.md)
* [Synthetic Monitoring Flow](docs/journeys/pager_duty/synthetic_monitoring_flow.md)
</stories_and_journeys>

---

## 6. Testing Mandate
If you create a new plugin, you **MUST** update `daemons/pager_duty/test_generalized_monitor.py` with an isolated, aggressively mocked test verifying its successful parsing and failure states. Headless APIs and synthetic execution layers safely suppress i18n translation requirements during tests. [@ANCHOR: synthetic_i18n]
