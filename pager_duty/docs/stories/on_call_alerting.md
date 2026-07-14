# Story: The Midnight Guardian

## Persona
**Alice**, a Senior Site Reliability Engineer who is currently "on-call" for the company's production infrastructure.

## Context
It's 2:00 AM on a Tuesday. Alice is fast asleep, but the `pager_duty` module is wide awake, monitoring the health of the primary Odoo cluster.

## The Event
A sudden database lock contention causes the Odoo XML-RPC interface to stop responding. The `generalized_monitor.py` daemon, running in its isolated execution loop, detects the failure during its "WSGI HTTP Ping" check.

1.  **Detection:** The daemon attempts a ping and fails. It recognizes that the system is down.
2.  **Notification:** The daemon calls the Odoo RPC to report the incident. Because the internal XML-RPC might be flaky, it also has direct SMTP fallbacks. In Odoo, the `report_incident` method is triggered [@ANCHOR: report_incident_rate_limit], utilizing an atomic Redis-based rate limit to prevent alert floods [@ANCHOR: pd_redis_rate_limit].

3.  **On-Duty Lookup:** The system queries the `calendar.event` model to find out who is currently assigned to the "Pager Duty Shift" [@ANCHOR: test_pager_notification]. It finds Alice's record because her shift was marked with `is_pager_duty=True`.
4.  **Alerting Alice:** An urgent message is posted to the incident chatter, and a notification is dispatched to Alice's mobile device via the mail service.

## Resolution
Alice wakes up to the alert. She logs into the NOC Dashboard [@ANCHOR: pager_board_data] and sees the "Critical" incident at the top of the list.

1.  **Acknowledgement:** Alice clicks the "Acknowledge" button. This action transitions the incident status and stops further escalation [@ANCHOR: action_acknowledge_incident].

2.  **Investigation:** She uses the integrated Log Analyzer [@ANCHOR: test_log_analyzer_views] to tail the production logs and identifies the offending SQL query.
3.  **Fix:** Alice kills the long-running transaction.
4.  **Verification:** The `generalized_monitor.py` daemon completes its next check cycle. Finding the system healthy again, it triggers the auto-resolution sequence [@ANCHOR: auto_resolve_incidents], closing Alice's ticket and logging her MTTR (Mean Time To Resolve) for the morning's post-mortem.
