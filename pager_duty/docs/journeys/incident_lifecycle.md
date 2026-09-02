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
  [@ANCHOR: pager_duty_info_alias_claim] for info@'s crm-collision-safe claim), both pointed at
  `pager.incident`.
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

## 1b. Trend Detection for Sub-Critical Occurrences
- **Severity Gate:** `report_incident()` [@ANCHOR: pager_trend_severity_gate] only pages on-duty
  immediately for `high`/`critical` severity (today's original behavior, unchanged). `low`/`medium`
  occurrences [@ANCHOR: pager_trend_detection_params] -- exactly the ones a human on-call engineer
  would otherwise deprioritize on manual triage -- are still recorded as a `pager.incident`
  (visible on the NOC board, `occurrence_count`/`last_occurred` accumulate as usual) but do NOT
  post a chatter notification or page anyone.
- **Rolling Window:** Each dedup match [@ANCHOR: pager_trend_window_update] updates
  `window_occurrence_count`/`window_start` on the existing incident -- a simple rolling-window
  counter (5 occurrences within 60 minutes, by default) distinct from the lifetime
  `occurrence_count`, so it measures a real burst rate rather than a cumulative total. The window
  resets to 1 whenever an occurrence arrives after the previous window has lapsed.
- **Trend Escalation:** [@ANCHOR: pager_trend_detection] Once a `low`/`medium` source's
  `window_occurrence_count` crosses the threshold within the window, `_raise_trend_incident()`
  [@ANCHOR: pager_raise_trend_incident] creates a real, separate, always-`high`-severity
  `Trend: <source>` incident describing the burst (count, window bounds, link back to the original
  incident) and pages on-duty for it -- turning an accumulating pattern that would otherwise stay
  silent into a real, paging incident, distinct from any individual occurrence. `trend_raised` on
  the original incident prevents raising a second trend incident for the same burst.

## 2. Notification & Assignment
- **Creation:** A new `pager.incident` record is created.
- **Calendar Query:** The system calls `get_current_on_duty_admin()` [@ANCHOR: test_pager_notification].
- **Dispatch:** If an engineer is on-call (`is_pager_duty=True` on their `calendar.event`) *and* the
  incident's own severity isn't gated by 1b above, they are added to the notification list.
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
