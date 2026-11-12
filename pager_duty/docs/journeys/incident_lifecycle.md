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
  `message_update()`. Before creating the incident, `message_new()` judges the message once with
  `inbound_spam_reasons()` (hams_helpdesk; the parked header facts and HTML shape of the raw mail, the SES verdicts, and whether the
  sender is a member). A quarantined message is created already `resolved` (`time_resolved` set) with the context key
  `pager_quarantined_spam`, so it is never on the board's open list and `create()`/`write()` send no `update_board` message; the
  reasons travel to the helpdesk adapter in the context key `hams_inbound_spam_reasons` (a list, empty meaning "judged, clean"),
  which routes the mirrored ticket to the `spam` stage with no calendar block. A person who moves that ticket back to New does not
  reopen the incident.
- **Bounce Filtering:** Before any of the above runs, `hams_base`'s `mail.thread.message_route()`
  override drops genuine DSN bounces, vacation auto-replies, and unsubscribe-intent messages sent to
  postmaster@ (the same filtering the dedicated bounce alias already had) -- only a genuine inquiry
  reaches the alias resolution step above.

## 1b. Trend Detection for Sub-Critical Occurrences
- **Severity Gate:** `report_incident()` (the gate, `pager_trend_severity_gate`, is described in the function reference at the end) only pages on-duty
  immediately for `high`/`critical` severity (today's original behavior, unchanged). `low`/`medium`
  occurrences (the thresholds are `pager_trend_detection_params`) -- exactly the ones a human on-call engineer
  would otherwise deprioritize on manual triage -- are still recorded as a `pager.incident`
  (visible on the NOC board, `occurrence_count`/`last_occurred` accumulate as usual) but do NOT
  post a chatter notification or page anyone.
- **Rolling Window:** Each dedup match (`pager_trend_window_update`) updates
  `window_occurrence_count`/`window_start` on the existing incident -- a simple rolling-window
  counter (5 occurrences within 60 minutes, by default) distinct from the lifetime
  `occurrence_count`, so it measures a real burst rate rather than a cumulative total. The window
  resets to 1 whenever an occurrence arrives after the previous window has lapsed.
- **Trend Escalation:** (fields: `pager_trend_detection`) Once a `low`/`medium` source's
  `window_occurrence_count` crosses the threshold within the window, `_raise_trend_incident()`
  creates a real, separate, always-`high`-severity
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
- **Auto-Resolve:** It calls `auto_resolve_incidents(source)` (see the function reference at the end).
- **Finalization:** The `status` moves to `resolved`. `time_resolved` is recorded.
- **Metric Computation:** `mttr` (Mean Time To Resolve) is calculated as the delta between `create_date` and `time_resolved`.
- **Board Cleanup:** The incident moves from the "Active" to "Resolved" section on the NOC Dashboard (`get_board_data`, see the function reference at the end).

## 5. Performance Optimization
- **Data Retrieval:** High-performance dashboard retrieval is handled via the `pager_get_board_data` Postgres procedure (installation described in the function reference at the end).

## 6. Metric Computation and Write-Time Guards

- **create() / write():** Both `create()` [@ANCHOR: incident_create] and `write()` [@ANCHOR: incident_write] on `pager.incident` are where the bus notification (`update_board`) and, on `write`, the timestamp bookkeeping for status transitions actually happen -- every state change described in sections 3 and 4 above routes through one of these two methods rather than the NOC Dashboard polling for changes itself. Neither sends `update_board` when the context key `pager_quarantined_spam` is true: an incident created from a message the inbound-mail filter quarantined is born resolved and is not announced to open boards (it is the only caller that sets the key).

- **On-Duty Notification:** `_notify_on_duty()` [@ANCHOR: notify_on_duty] is the method that performs the calendar lookup and chatter post described in section 2 -- it only actually runs for `high`/`critical` severity per the trend-detection gate in section 1b, so a `low`/`medium` occurrence never triggers a real notification by itself.

- **MTTA:** `_compute_mtta()` is the stored computed field backing the "Metric Computation" step in section 3 -- the delta between `create_date` and `time_acknowledged`.

- **MTTR:** `_compute_mttr()` is the equivalent computed field for section 4's resolution metric -- the delta between `create_date` and `time_resolved`.

## 7. Helpdesk Ticket Escalation

- **Ticket Creation:** When an incident is configured to also open a support ticket (Bob's "Helpdesk Integration" toggle from `docs/stories/automated_monitoring_setup.md`), the adapter's own `create()` override [@ANCHOR: incident_ticket_adapter_create] is what actually creates the linked `helpdesk.ticket` record and the event tying it back to the originating incident.

- **SMTP Fallback:** If the configured helpdesk model itself can't be resolved (a misconfigured system parameter, or the Helpdesk module not installed), `_execute_smtp_fallback()` [@ANCHOR: execute_smtp_fallback] sends the ticket content as a plain email instead of silently dropping it -- the same "warn and degrade, don't go silent" philosophy as the daemon's own SMTP fallback in section 3's monitoring loop.

## 8. AI Triage Assist

- **Listing:** An on-call AI triage assistant calls `mcp_list_incidents()` to find open incidents, backed by the narrowly-scoped, read-only MCP triage service account described in `docs/stories/on_call_alerting.md`.

- **Reading:** The assistant calls `mcp_get_incident_detail()` to read one incident's full chatter history.

- **Leaving Notes:** `mcp_add_note()` [@ANCHOR: mcp_add_note] is the one write the assistant is allowed to make -- posting a triage note to the incident's chatter, tagged as AI-authored, without ever being able to acknowledge, resolve, or otherwise mutate the incident's own state.

## Function reference

The model methods and constants this journey relies on, one entry each (`models/incident.py` unless stated). The write-time guards (`create`, `write`, `_notify_on_duty`, `mcp_add_note`) are documented where they are cited above.

### `TREND_WINDOW_MINUTES`, `TREND_OCCURRENCE_THRESHOLD`, `TREND_TRACKED_SEVERITIES` [@ANCHOR: pager_trend_detection_params]

Module constants at the top of `models/incident.py` that define trend detection. `TREND_WINDOW_MINUTES = 60` is the length of the rolling window in minutes; `TREND_OCCURRENCE_THRESHOLD = 5` is the count within one window that raises a trend incident; `TREND_TRACKED_SEVERITIES = ("low", "medium")` lists the severities that do not page at once but are counted. `high` (the usual default) and `critical` are not tracked and page immediately. They are plain module values, changed only by editing the code (there is no system parameter), and are read by `report_incident`, `_repage_if_due` and `_raise_trend_incident`. Related: `REPAGE_HOURS = 6`, the re-page interval for paging incidents. Test: `test_11_low_severity_does_not_page_but_high_severity_still_does` in `tests/test_incident.py`.

### Severity gate in `report_incident(vals)` [@ANCHOR: pager_trend_severity_gate]

After a new incident is created, `report_incident` calls `_notify_on_duty` only when `vals["severity"]` is not in `TREND_TRACKED_SEVERITIES`, so `high` and `critical` post the "New Incident Created" chatter note and page the on-duty administrator, while `low` and `medium` are recorded and shown on the NOC Board but make no notification. Input: the incident value dict (`source`, `severity`, `description`, optional `website_id`, `units`). Output of the method: the incident id, or `False` when the rate limit suppressed the report. The gate applies only to newly created incidents; a repeat of an open one goes through the window update and the re-page rule instead. It also guards the unit-ticket join: only paging severities join per-unit tickets. Tests: `test_11_low_severity_does_not_page_but_high_severity_still_does` in `tests/test_incident.py`, `test_04_spam_flagged_email_incident_is_quarantined_not_dropped` in `tests/test_helpdesk_adapter.py`.

### Rolling-window update in `report_incident(vals)` [@ANCHOR: pager_trend_window_update]

When `report_incident` finds an open or acknowledged incident for the same source (and website), it does not create another. It adds 1 to `occurrence_count`, sets `last_occurred` to now and updates the window: if `window_start` is empty or more than `TREND_WINDOW_MINUTES * 60` seconds ago, the window restarts (`window_start` is set to now and `window_occurrence_count` to 1); otherwise `window_occurrence_count` grows by 1. If the existing incident's severity is tracked, `trend_raised` is false and the new window count has reached `TREND_OCCURRENCE_THRESHOLD`, it calls `_raise_trend_incident`. It then calls `_repage_if_due` and returns the existing id. One `write` on the incident; no lock beyond the row update, so two simultaneous reports can each count (the Redis rate limit makes that rare). Test: `test_12_trend_window_resets_after_it_lapses`.

### Trend fields `window_start`, `window_occurrence_count`, `trend_raised` [@ANCHOR: pager_trend_detection]

Three fields of `pager.incident` that hold the trend state, added next to `occurrence_count` and `last_occurred`. `window_start` (`Datetime`, default now) is when the current window began; `window_occurrence_count` (`Integer`, default 1) counts reports since then and, unlike the lifetime `occurrence_count`, resets when the window lapses; `trend_raised` (`Boolean`, default false) is set once a trend incident exists, so a burst raises only one. They are written by `report_incident` (window) and `_raise_trend_incident` (`trend_raised`) and read by the trend check. Stored in the `pager_incident` table, no constraint, no serialized form. Existing rows read as one occurrence in a fresh window. Test: `test_11_low_severity_does_not_page_but_high_severity_still_does`; the behaviour is in `test_13`.

### `_raise_trend_incident(self, source_incident, website_id)` [@ANCHOR: pager_raise_trend_incident]

Turns a burst of silent low or medium reports into a real page. Arguments: `source_incident` (the open `pager.incident` whose window crossed the threshold) and `website_id` (`int` or `False`). As the `user_pager_incident_creator` service account it creates a new incident named and sourced `Trend: <original source>` with severity `high`, whose description gives the count, the window bounds, the threshold and the id of the original; it does not call `report_incident`, so the `Trend:` source avoids deduplication against the original. It then sets `trend_raised` on the original and calls `_notify_on_duty` with "New Incident Created (trend escalation)". Returns the new incident's id. Raises the usual access errors if the service account lacks rights. It creates a record and may post chatter and a notification. Test: `test_13_trend_threshold_raises_a_real_paging_incident_exactly_once`.

### `auto_resolve_incidents(self, source, website_id=None)` [@ANCHOR: auto_resolve_incidents]

Closes the open incidents of one source when it recovers (`@public_rpc @api.model`). Arguments: `source` (`str`, the check name) and `website_id` (`int` or `None`, default the context `website_id`; when set only that website's incidents match). It requires the caller to be the superuser environment, a service account (`is_service_account`) or a member of `pager_duty.group_pager_admin`; anyone else gets `AccessError`. As the `user_pager_service_internal` account it searches up to 1000 incidents of that source in status `open` or `acknowledged`, writes `status = resolved` (which stamps `time_resolved`, so `mttr` is computed, and notifies the board) and posts "Auto-resolved by NOC monitor recovery sequence." on each as the mail service account. Returns `True` even when nothing matched. Idempotent. Test: `test_02_zero_sudo_impersonation_and_mail_standard` in `tests/test_incident.py`.

### `get_board_data(self)` [@ANCHOR: pager_board_data]

Returns the data the NOC Board draws (`@public_rpc @api.model`). It takes no arguments; website comes from the context or the current website, company from the environment. It runs `SELECT pager_get_board_data(website_id, company_id)` and returns the JSON object with `on_duty`, `active` (open and acknowledged, newest first, 50 at most), `resolved` (10 most recent, so a recovered incident moves here) and `stats` (checks per status). Rows are limited to the website and company. Read only; a database error propagates. See `docs/journeys/daemon_execution_loop.md` for the full field list. Test: `test_01_board_data_rpc` in `tests/test_board_edge_cases.py`.

### `_install_postgres_procedures(env)` and `pager_get_board_data` [@ANCHOR: pager_duty_postgres_procedures]

`hooks.py` holds the SQL of the PostgreSQL function `pager_get_board_data(p_website_id, p_company_id)` that returns the whole board as one `jsonb` value, and `_install_postgres_procedures(env)` runs it (`CREATE OR REPLACE FUNCTION`) from the module's post-init hook after flushing the environment. The statement runs inside a savepoint, and a failure is logged as an error and not raised, so the module still installs. The function finds the on-duty user from `calendar_event` rows with `is_pager_duty` covering now (the user must belong to the company), the active and resolved incident lists and the check counts per status, all filtered by website and company. `data/procedures.xml` holds only a vestigial server action pointing here. Test: `test_07_board_data_procedure` in `tests/test_incident.py`.

### `_compute_mtta(self)` [@ANCHOR: compute_mtta]

The stored computed field behind the `mtta` column ("MTTA (Minutes)"). It depends on `time_acknowledged` and `create_date`. For each record with both values it stores `(time_acknowledged - create_date)` in minutes as a float; otherwise `0.0`. No arguments or return value (it assigns the field). It is recomputed when `write` stamps `time_acknowledged` on the first acknowledgement; a second acknowledge does not re-stamp, so the metric is not overwritten. No I/O or locks. Test: `test_05_mtta_mttr_calculation` in `tests/test_incident.py`.

### `_compute_mttr(self)` [@ANCHOR: compute_mttr]

The stored computed field behind `mttr` ("MTTR (Minutes)"), the counterpart of `_compute_mtta`. It depends on `time_resolved` and `create_date`; for a record with both it stores the difference in minutes as a float, else `0.0`. Reopening is not modelled: `time_resolved` is stamped only on the first transition to `resolved`. Assigns the field, returns nothing, no I/O. The figure is what the post-mortem reads. Test: `test_05_mtta_mttr_calculation` in `tests/test_incident.py`.

### `mcp_list_incidents(self, status=None, severity=None, limit=50)` [@ANCHOR: mcp_list_incidents]

The read-only list the MCP `list_incidents` tool calls (`@public_rpc @api.model`), callable by the narrow triage service account. Arguments: `status` (`open`, `acknowledged` or `resolved`, optional), `severity` (optional) and `limit` (default 50, never more than 200). Returns a list of dicts, newest first: `id`, `untrusted_block` (the name and source, filtered, inside a random-delimiter block, or a withheld notice when the content looks like an injection), `suspicious`, `findings`, `severity`, `status`, `occurrence_count`, `is_escalated`, `create_date` and `last_occurred` (ISO strings). Free text a stranger could have written never leaves unfiltered. No writes. Test: `test_01_mcp_list_incidents_filters_by_status_and_severity` in `tests/test_pager_mcp_triage.py`.

### `mcp_get_incident_detail(self)` [@ANCHOR: mcp_get_incident_detail]

Full detail of one incident for the MCP `get_incident` tool (`@public_rpc`, on a single record; the caller passes `ids=[id]`). No other arguments. It reads the chatter as the mail service account (so the triage account needs no `mail.message` right), ordered by id, and keeps only comments and notifications, at most about 400 lines. The name, source, description and each message and author are filtered and wrapped in one untrusted block. Returns a dict: `id`, `untrusted_data_notice`, `untrusted_block` (or a withheld notice when `suspicious`), `suspicious`, `findings`, `attachments_notice`, `severity`, `status`, `occurrence_count`, `is_escalated`, `create_date`, `last_occurred`. Raises if called on more than one record (`ensure_one`). Attachments are never read. Tests: `test_02_mcp_get_incident_detail_includes_chatter`, `test_02b_a_hostile_log_line_in_an_incident_is_filtered_and_withheld` in `tests/test_pager_mcp_triage.py`.
