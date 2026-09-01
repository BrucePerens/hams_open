# Journey: Incident Lifecycle

This journey tracks the technical state transitions of an incident from initial detection to final resolution.

## 1. Detection & Rate Limiting
- **Trigger:** The `generalized_monitor.py` daemon detects a failure.
- **RPC Call:** The daemon calls `report_incident(vals)`.
- **Throttling:** The method checks Redis for a `pager_rate_limit:<source>` key [@ANCHOR: report_incident_rate_limit]. If found, the incident is suppressed to prevent alert storms.
- **De-duplication:** If no rate limit exists, it searches for existing open or acknowledged incidents with the same `source`.

## 1a. Detection via Inbound Email (info@/postmaster@hams.com)
- **Trigger:** A real inbound email arrives at info@hams.com or postmaster@hams.com (SES -> S3 ->
  the mail-ingest daemon -> `hams_helpdesk.ticket.ingest_inbound_email()` -> Odoo's own
  `mail.thread.message_process()`; see `docs/proposals/EMAIL_SEND_RECEIVE.md`).
- **Alias Resolution:** Odoo's own mailgateway resolves the recipient against the `info`/
  `postmaster` `mail.alias` records (`data/mail_alias_data.xml`, `hooks.py`'s `_claim_info_alias()`
  for info@'s crm-collision-safe claim), both pointed at `pager.incident`.
- **Record Creation:** Since the message doesn't match an existing thread, `message_new()`
  [@ANCHOR: pager_incident_message_new] creates the incident directly -- deliberately bypassing
  `report_incident()`'s rate-limit/dedup (designed for repeated automated signals, not distinct
  human inquiries). `source` is built per-sender (`{prefix}:{sender email}`), so each correspondent
  gets their own incident thread; real replies thread onto it natively via Odoo's own
  `message_update()`.
- **Bounce Filtering:** Before any of the above runs, `hams_base`'s `mail.thread.message_route()`
  override drops genuine DSN bounces, vacation auto-replies, and unsubscribe-intent messages sent to
  postmaster@ (the same filtering the dedicated bounce alias already had) -- only a genuine inquiry
  reaches the alias resolution step above.

## 2. Notification & Assignment
- **Creation:** A new `pager.incident` record is created.
- **Calendar Query:** The system calls `get_current_on_duty_admin()` [@ANCHOR: test_pager_notification].
- **Dispatch:** If an engineer is on-call (`is_pager_duty=True` on their `calendar.event`), they are added to the notification list.
- **Communication:** An internal message is posted to the incident chatter via the `mail_service_internal` service account.

## 3. Acknowledgement & Escalation
- **User Action:** The engineer clicks "Acknowledge" [@ANCHOR: action_acknowledge_incident].
- **State Change:** `status` moves to `acknowledged`. `time_acknowledged` and `acknowledged_by_id` are recorded.
- **Metric Computation:** `mtta` (Mean Time To Acknowledge) is calculated as the delta between `create_date` and `time_acknowledged`.
- **Bus Update:** An `update_board` signal is sent via `bus.bus` to refresh the NOC Dashboard.

## 4. Recovery & Resolution
- **System Recovery:** The `generalized_monitor.py` daemon detects the check is passing again.
- **Auto-Resolve:** It calls `auto_resolve_incidents(source)` [@ANCHOR: auto_resolve_incidents].
- **Finalization:** The `status` moves to `resolved`. `time_resolved` is recorded.
- **Metric Computation:** `mttr` (Mean Time To Resolve) is calculated as the delta between `create_date` and `time_resolved`.
- **Board Cleanup:** The incident moves from the "Active" to "Resolved" section on the NOC Dashboard [@ANCHOR: pager_board_data].

## 5. Performance Optimization
- **Data Retrieval:** High-performance dashboard retrieval is handled via the `pager_get_board_data` Postgres procedure. [@ANCHOR: pager_duty_postgres_procedures]
