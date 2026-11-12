# Data structures

### `BrowserExceptionApi` [@ANCHOR: hams_helpdesk:hams_helpdesk_browser_exception_api_browser_exception_api]

**Purpose and location.** The class has no docstring; from its definition it provides `_is_same_origin`, `report`. Defined in `hams_helpdesk/controllers/browser_exception_api.py:47` as a class; bases: `http.Controller`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_is_same_origin`, `report`.

**See also:** test `test_the_unit_timeout_outlasts_the_wait (daemon/test_exception_reporter.py)`.

### `GithubWebhookController` [@ANCHOR: hams_helpdesk:hams_helpdesk_github_webhook_api_github_webhook_controller]

**Purpose and location.** The class has no docstring; from its definition it provides `receive_github_webhook`, `_handle_dependabot_alert`, `_handle_workflow_run`. Defined in `hams_helpdesk/controllers/github_webhook_api.py:79` as a class; bases: `http.Controller`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `receive_github_webhook`, `_handle_dependabot_alert`, `_handle_workflow_run`.

**See also:** test `test_github_webhook_wrong_key_signature_is_denied (tests/test_github_webhook.py)`.

### `HelpdeskPortal` [@ANCHOR: hams_helpdesk:hams_helpdesk_portal_helpdesk_portal]

**Purpose and location.** The class has no docstring; from its definition it provides `_prepare_home_portal_values`, `portal_my_tickets`, `portal_ticket_detail`, `portal_ticket_close`, `portal_ticket_new`, `portal_ticket_submit`. Defined in `hams_helpdesk/controllers/portal.py:23` as a class; bases: `CustomerPortal`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_prepare_home_portal_values`, `portal_my_tickets`, `portal_ticket_detail`, `portal_ticket_close`, `portal_ticket_new`, `portal_ticket_submit`.

**See also:** test `test_portal_ticket_new_renders_without_callsign (tests/test_ui_tours.py)`.

### `ReportError` [@ANCHOR: hams_helpdesk:hams_helpdesk_exception_reporter_report_error]

**Purpose and location.** The report could not be delivered. The message never holds a key, URL or response body. Defined in `hams_helpdesk/daemon/exception_reporter.py:61` as a class; bases: `Exception`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_a_rejected_key_is_not_retried (daemon/test_exception_reporter.py)`.

### `OdooUnavailable` [@ANCHOR: hams_helpdesk:hams_helpdesk_exception_reporter_odoo_unavailable]

**Purpose and location.** Odoo did not answer (connection refused or timed out, or a proxy's 502/503/504): worth retrying. Defined in `hams_helpdesk/daemon/exception_reporter.py:65` as a class; bases: `ReportError`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_gives_up_after_the_budget_with_the_last_error (daemon/test_exception_reporter.py)`.

### `BrowserException` [@ANCHOR: hams_helpdesk:hams_helpdesk_browser_exception_browser_exception]

**Purpose and location.** Model description: Browser JavaScript error reports. Defined in `hams_helpdesk/models/browser_exception.py:40` as an Odoo model; bases: `models.AbstractModel`. (model `hams_helpdesk.browser.exception`.) It is an abstract model: it has no table and is only mixed into others.

**Fields.** none declared by this class.

**Class attributes.** `_name = "hams_helpdesk.browser.exception"`; `_description = "Browser JavaScript error reports"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_client_key`, `_rate_limited`, `_current_versions`, `_release_is_current`, `_record`.

**See also:** test `test_a_failure_inside_the_beacon_is_not_reported_by_the_beacon (tests/test_browser_exceptions.py)`.

### `ReportRejected` [@ANCHOR: hams_helpdesk:hams_helpdesk_browser_exception_schema_report_rejected]

**Purpose and location.** The report is refused. ``status`` is the HTTP status to answer, ``reason`` a short fixed word (never a piece of the report) for the log and the tests. Defined in `hams_helpdesk/models/browser_exception_schema.py:64` as a class; bases: `Exception`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`.

**See also:** test `test_each_bad_report_is_refused_with_its_own_reason (tests/test_relay_exceptions.py)`.

### `CalendarEvent` [@ANCHOR: hams_helpdesk:hams_helpdesk_calendar_event_calendar_event]

**Purpose and location.** Extends the Odoo model "calendar.event" with the fields `helpdesk_ticket_ids` and the methods `get_current_on_duty_admin`, `get_upcoming_duty_shifts`. Defined in `hams_helpdesk/models/calendar_event.py:7` as an Odoo model; bases: `models.Model`. (extends `calendar.event`.)

| Field | Type | Meaning |
|---|---|---|
| `helpdesk_ticket_ids` | `One2many` | Helpdesk Tickets (comodel `hams_helpdesk.ticket`) |

**Class attributes.** `_inherit = "calendar.event"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `get_current_on_duty_admin`, `get_upcoming_duty_shifts`.

**See also:** test `test_01_ticket_creation_and_routing (tests/test_helpdesk_core.py)`.

### `ExceptionFingerprintDaemon` [@ANCHOR: hams_helpdesk:hams_helpdesk_daemon_exception_exception_fingerprint_daemon]

**Purpose and location.** Extends the Odoo model "hams_helpdesk.exception.fingerprint" with no new fields and the methods `report_daemon_failure`, `_report_unit_failures`. Defined in `hams_helpdesk/models/daemon_exception.py:51` as an Odoo model; bases: `models.Model`. (extends `hams_helpdesk.exception.fingerprint`.)

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "hams_helpdesk.exception.fingerprint"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `report_daemon_failure`, `_report_unit_failures`.

**See also:** test `test_a_daemon_failure_always_reopens (tests/test_exception_tickets.py)`.

### `ExceptionFingerprint` [@ANCHOR: hams_helpdesk:hams_helpdesk_exception_fingerprint_exception_fingerprint]

**Purpose and location.** Model description: Server exception fingerprint. Defined in `hams_helpdesk/models/exception_fingerprint.py:286` as an Odoo model; bases: `models.Model`. (model `hams_helpdesk.exception.fingerprint`; default order "last_seen desc, id desc".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Failure (required=`True`; readonly=`True`) |
| `fingerprint` | `Char` | no label or help in the source (required=`True`; index=`True`; readonly=`True`) |
| `exc_type` | `Char` | Exception (readonly=`True`) |
| `module` | `Char` | no label or help in the source (index=`True`; readonly=`True`) |
| `frame` | `Char` | Innermost frame (readonly=`True`) |
| `route` | `Char` | Route (index=`True`; readonly=`True`) |
| `source` | `Selection` | no label or help in the source (values `http`, `rpc`, `cron`, `mail`, `daemon`, `browser`, `relay`; readonly=`True`) |
| `occurrence_count` | `Integer` | Occurrences (readonly=`True`) |
| `first_seen` | `Datetime` | no label or help in the source (readonly=`True`) |
| `last_seen` | `Datetime` | no label or help in the source (index=`True`; readonly=`True`) |
| `last_written_at` | `Datetime` | no label or help in the source (readonly=`True`) |
| `notified_at` | `Datetime` | When opening this row's ticket paged the on-duty admin. Empty when it was opened silently. (readonly=`True`) |
| `last_user_id` | `Integer` | Last user id: Internal user id of the last logged-in user affected. Never a name or an email. (readonly=`True`) |
| `last_member_id` | `Many2one` | Last member: The last signed-in member this failure was reported for: a link to the account, never a copy of the name, callsign or email. (comodel `res.users`; index=`True`; readonly=`True`; ondelete=`"set null"`) |
| `member_ids` | `One2many` | Affected members (comodel `hams_helpdesk.exception.member`; readonly=`True`) |
| `unlisted_member_count` | `Integer` | Further members (not listed): Reports from members beyond the list's limit, counted only. (readonly=`True`) |
| `release_marker` | `Char` | Newest module-install timestamp when this last happened. When a closed ticket recurs under a different marker, it is reopened. (readonly=`True`) |
| `samples_json` | `Text` | Last sample traces (readonly=`True`) |
| `reporters_json` | `Text` | Reported by: JSON list of everything that reported this failure: pager check names, the exception reporter, the daemon itself. Never message text. (readonly=`True`) |
| `ticket_id` | `Many2one` | Ticket (comodel `hams_helpdesk.ticket`; index=`True`; readonly=`True`; ondelete=`"set null"`) |
| `ticket_stage` | `Selection` | Ticket stage (related=`"ticket_id.stage"`; store=`True`) |

**Class attributes.** `_name = "hams_helpdesk.exception.fingerprint"`; `_description = "Server exception fingerprint"`; `_order = "last_seen desc, id desc"`; `_fingerprint_unique = models.Constraint(
        "UNIQUE(fingerprint)", "One row per exception fingerprint."
    )`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_release_marker`, `_reporters`, `_ticket_body`, `_record_occurrence`, `_reopen_reason`, `_sync_ticket`, `_announce_reopen`, `_notification_state`.

**See also:** test `test_a_signed_in_member_is_linked_to_the_fingerprint_and_the_ticket (tests/test_exception_members.py)`.

### `ExceptionMember` [@ANCHOR: hams_helpdesk:hams_helpdesk_exception_member_exception_member]

**Purpose and location.** Model description: Member affected by an automatic exception ticket. Defined in `hams_helpdesk/models/exception_member.py:52` as an Odoo model; bases: `models.Model`. (model `hams_helpdesk.exception.member`; default order "last_seen desc, id desc".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | A number only; the member's name is never part of it. (compute=`"_compute_name"`) |
| `fingerprint_id` | `Many2one` | no label or help in the source (comodel `hams_helpdesk.exception.fingerprint`; required=`True`; index=`True`; readonly=`True`; ondelete=`"cascade"`) |
| `ticket_id` | `Many2one` | mirrors the related field `"fingerprint_id.ticket_id"` (comodel `hams_helpdesk.ticket`; related=`"fingerprint_id.ticket_id"`; store=`True`; index=`True`; readonly=`True`) |
| `user_id` | `Many2one` | Member account: A link to the account, never a copy of the name, callsign or email. (comodel `res.users`; index=`True`; readonly=`True`; ondelete=`"set null"`) |
| `first_seen` | `Datetime` | no label or help in the source (readonly=`True`) |
| `last_seen` | `Datetime` | no label or help in the source (readonly=`True`) |
| `occurrence_count` | `Integer` | Reports: Reports this member was part of. A lower bound: repeats held back by the once-a-minute gate cannot be told apart by member and are only counted on the ticket. (readonly=`True`) |
| `member_name` | `Char` | Member (compute=`"_compute_details"`) |
| `callsigns` | `Char` | Callsign(s) (compute=`"_compute_details"`) |
| `contact_kind` | `Selection` | no label or help in the source (selection `[
            (KIND_MEMBER, "The member"),
            (KIND`; compute=`"_compute_details"`) |
| `contact_label` | `Char` | Write to (compute=`"_compute_details"`) |
| `contact_name` | `Char` | Contact name (compute=`"_compute_details"`) |
| `contact_email` | `Char` | Email (compute=`"_compute_details"`) |

**Class attributes.** `_name = "hams_helpdesk.exception.member"`; `_description = "Member affected by an automatic exception ticket"`; `_order = "last_seen desc, id desc"`; `_member_once = models.Constraint(
        "UNIQUE(fingerprint_id, user_id)", "A member is listed once per exception`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_reportable_member_uid`, `_note_member`, `_is_listed`, `_contact_user`, `_callsigns_of`, `_compute_name`, `_compute_details`, `_contact_partner`, `action_reply_to_member`, `action_mailto`.

**See also:** test `test_a_signed_in_member_is_linked_to_the_fingerprint_and_the_ticket (tests/test_exception_members.py)`.

### `HelpdeskTicket` [@ANCHOR: hams_helpdesk:hams_helpdesk_helpdesk_ticket_helpdesk_ticket]

**Purpose and location.** Model description: Helpdesk Ticket. Defined in `hams_helpdesk/models/helpdesk_ticket.py:114` as an Odoo model; bases: `models.Model`. (model `hams_helpdesk.ticket`; inherits ["hams_helpdesk.untrusted.mixin", "mail.thread", "mail.activity.mixin"].)

| Field | Type | Meaning |
|---|---|---|
| `suspicion_score` | `Integer` | Suspicion score (default=`0`; index=`True`; copy=`False`) |
| `suspicious` | `Boolean` | Suspicious: Hidden or injected content was found. Never auto-triaged, auto-answered or acted on by an AI. (default=`False`; index=`True`; copy=`False`) |
| `suspicion_removed` | `Integer` | Hidden characters removed (default=`0`; copy=`False`) |
| `suspicion_log_ids` | `One2many` | Filter findings (comodel `hams_helpdesk.ticket.suspicion`; copy=`False`) |
| `name` | `Char` | Subject (required=`True`) |
| `description` | `Html` | Description |
| `callsign` | `Char` | Callsign: Relevant amateur radio callsign. |
| `ticket_type` | `Selection` | Ticket Type (default=`"general"`) |
| `active` | `Boolean` | no label or help in the source (default=`True`) |
| `exception_fingerprint_ids` | `One2many` | Exception fingerprints (comodel `hams_helpdesk.exception.fingerprint`; readonly=`True`) |
| `affected_member_ids` | `One2many` | Reporting member(s) (comodel `hams_helpdesk.exception.member`; readonly=`True`) |
| `affected_member_count` | `Integer` | Members (compute=`"_compute_affected_members"`) |
| `latest_member_label` | `Char` | Latest member (compute=`"_compute_affected_members"`) |
| `user_id` | `Many2one` | Assigned To (comodel `res.users`; index=`True`) |
| `partner_id` | `Many2one` | Customer (comodel `res.partner`; index=`True`) |
| `stage` | `Selection` | Stage (values `new`, `in_progress`, `resolved`, `closed`, `spam`; required=`True`; default=`"new"`) |
| `triage_verdict` | `Selection` | Triage verdict (values `spam`, `phishing`, `real`, `unsure`; index=`True`; readonly=`True`; copy=`False`) |
| `triage_verdict_source` | `Selection` | Verdict by (values `rules`, `agent`; readonly=`True`; copy=`False`) |
| `triage_reasons` | `Text` | Verdict reasons (readonly=`True`; copy=`False`) |
| `triage_verdict_at` | `Datetime` | Verdict at (readonly=`True`; copy=`False`) |
| `triage_overridden` | `Boolean` | Verdict overridden by a person: A person moved the ticket out of the Spam stage after a spam or phishing verdict. The agent never judges the ticket again, and the verdict, its source and this flag together are the labelled example (a false positive) the rule weights and the agent's prompt are tuned from. No ticket text is kept. (index=`True`; readonly=`True`; copy=`False`) |
| `triage_overridden_at` | `Datetime` | Overridden at (readonly=`True`; copy=`False`) |
| `triage_delay_seconds` | `Integer` | Seconds to verdict: Whole seconds from the ticket's creation to the AI triage agent's verdict: the review delay the stall alarm watches. (readonly=`True`; copy=`False`) |
| `triage_stalled_at` | `Datetime` | Stall alarm raised: When the stall alarm fired for this ticket (no verdict within the configured time). Set once. (readonly=`True`; copy=`False`) |
| `links_defanged` | `Boolean` | Links shown as text: True when the triage verdict is phishing and no person has moved the ticket out of Spam: the staff view and the notification email then show the ticket's links as plain text. (compute=`"_compute_links_defanged"`) |
| `description_defanged` | `Html` | Description (links as text): The description as the staff form shows it for a ticket whose links are defanged; the description itself otherwise. (compute=`"_compute_description_defanged"`; readonly=`True`) |
| `notify_state` | `Selection` | Notification (values `sent`, `held`, `suppressed`; required=`True`; default=`"sent"`; index=`True`; readonly=`True`; copy=`False`) |
| `notify_release_at` | `Datetime` | Hold ends (index=`True`; readonly=`True`; copy=`False`) |
| `priority` | `Selection` | Priority (values `0`, `1`, `2`, `3`; default=`"0"`) |
| `calendar_event_id` | `Many2one` | Scheduled Event: Linked calendar event for incident response or scheduled assistance. (comodel `calendar.event`) |
| `website_id` | `Many2one` | Website: The website this ticket was created on. (comodel `website`; ondelete=`"restrict"`) |
| `company_id` | `Many2one` | Company (comodel `res.company`; required=`True`; default=`lambda self: self.env.company`) |
| `ncmec_recording_uuid` | `Char` | QSO Recording UUID: ham.simulated.band.qso.recording's own recording_uuid (ham_communications_consent, hams_com) -- a loose Char reference, not a Many2one, since that model may not be installed in every deployment of this module (see _QSO_RECORDING_MODEL's own comment). Also this ticket's own dedup key: _ncmec_report_ticket_for_recording attaches a second flag for the same recording to the existing ticket rather than opening a duplicate one. (index=`True`) |
| `ncmec_recording_playback_url` | `Char` | Recording Playback URL: The public /simulated-band/recordings/<uuid> playback page (qso_recording_portal.py, ham_communications_consent) -- anyone, including an NCMEC reviewer with no hams.com account, can open it directly under the proposal's own sunshine policy. Built once, at packet-assembly time, from web.base.url -- not recomputed later, so a subsequent change to web.base.url can't retroactively alter what a filed report pointed at. |
| `ncmec_reported_user_id` | `Many2one` | Reported User: The hams.com account apparently responsible for the flagged interaction, when known. Left empty is a normal, expected outcome for a human QSO leg -- see comm_consent_qso_recording.py's own operator_user_id field help text for why (the SFU's own /ws endpoint does not authenticate humans today). (comodel `res.users`; ondelete=`"set null"`) |
| `ncmec_legal_hold_applied` | `Boolean` | Legal Hold Applied: True once this ticket has successfully called ham.simulated.band.qso.recording.action_apply_legal_hold() on the linked recording (18 U.S.C. Sec. 2258A's one-year preservation duty, overriding the ordinary 30-day purge). See ncmec_legal_hold_note when this is False after creation -- it explains why, and whether an admin needs to apply the hold manually (action_ncmec_apply_legal_hold_manually below). (default=`False`) |
| `ncmec_legal_hold_note` | `Char` | Legal Hold Status: Set whenever the automatic legal-hold attempt at ticket-creation time did not succeed (no matching recording, ham_communications_consent not installed, or an AccessError -- see _ncmec_apply_recording_legal_hold_best_effort's own docstring for why an AccessError is the realistic, expected outcome today). Cleared on success. |
| `ncmec_contact_email` | `Char` | Reporting Contact (Email): hams.com's own designated NCMEC reporting contact, resolved from hams_helpdesk.ncmec_contact_email at packet-assembly time and stored here (not re-read live) so a later config change can't retroactively rewrite what a filed report said. |
| `ncmec_contact_phone` | `Char` | Reporting Contact (Phone) |
| `ncmec_report_state` | `Selection` | NCMEC Report Status: Prevents double-filing the same incident (section G's own requirement): once this is report_submitted or report_confirmed, action_ncmec_report refuses to submit again. emailed_fallback is deliberately NOT terminal -- see this field's own selection list. (values `not_reported`, `emailed_fallback`, `report_submitted`, `report_confirmed`; required=`True`; default=`"not_reported"`) |
| `ncmec_report_reference` | `Char` | NCMEC Report Reference: The report id NCMEC's own /submit response returns (real API path), or typed in by an admin after manually pasting the packet into NCMEC's own portal (fallback path). |
| `ncmec_report_submitted_at` | `Datetime` | Reported At |
| `ncmec_report_submitted_by_id` | `Many2one` | Reported By (comodel `res.users`; ondelete=`"set null"`) |
| `ncmec_report_packet` | `Text` | NCMEC Report Packet: The full assembled report packet (reported user info, recording playback URL, hams.com's own reporting-contact info) -- built once at ticket-creation time and shown read-only on the form so an admin can copy it into NCMEC's own portal if no API credentials are configured (see action_ncmec_report). |
| `github_source_url` | `Char` | GitHub Source URL: The originating GitHub dependabot alert's or workflow run's own html_url -- this ticket's own dedup key (see _github_webhook_ticket_for below): GitHub retries a webhook delivery on any non-2xx response and can occasionally redeliver even an already-processed one, so a second delivery for the same alert/run must not open a second ticket. (index=`True`) |

**Class attributes.** `_name = "hams_helpdesk.ticket"`; `_description = "Helpdesk Ticket"`; `_inherit = ["hams_helpdesk.untrusted.mixin", "mail.thread", "mail.activity.mixin"]`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_compute_affected_members`, `_onchange_partner_id`, `create`, `_schedule_triage_wakeup`, `_silent_exception_note`, `_automated_routing_and_notification`, `write`, `action_shift_handoff`, `action_portal_close`, `action_close`, `_cron_triage_stall_alarm`, `_triage_hold_seconds`, `_triage_notify_on_leaving_spam`, `_triage_release_notification`, `_cron_release_held_notifications`, `_triage_quarantine_verdicts`, `_triage_links_defanged`, `_compute_links_defanged`, `_compute_description_defanged`, `_notify_by_email_render_layout`, `_triage_record_verdict`, `_triage_record_override`, `_require_triage_caller`, `mcp_record_verdict`, `mcp_post_internal_note` and more.

**See also:** test `test_a_signed_in_member_is_linked_to_the_fingerprint_and_the_ticket (tests/test_exception_members.py)`.

### `IrCronExceptionReport` [@ANCHOR: hams_helpdesk:hams_helpdesk_ir_cron_exception_ir_cron_exception_report]

**Purpose and location.** Extends the Odoo model "ir.cron" with no new fields and the methods `_callback`. Defined in `hams_helpdesk/models/ir_cron_exception.py:17` as an Odoo model; bases: `models.Model`. (extends `ir.cron`.)

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "ir.cron"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_callback`.

**See also:** test `test_failing_cron_job_is_reported_and_still_raises (tests/test_exception_tickets.py)`.

### `IrHttpExceptionReport` [@ANCHOR: hams_helpdesk:hams_helpdesk_ir_http_exception_ir_http_exception_report]

**Purpose and location.** Extends the Odoo model "ir.http" with no new fields and the methods `_pre_dispatch`, `_match`, `_dispatch`, `_handle_error`. Defined in `hams_helpdesk/models/ir_http_exception.py:192` as an Odoo model; bases: `models.AbstractModel`. (extends `ir.http`.) It is an abstract model: it has no table and is only mixed into others.

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "ir.http"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_pre_dispatch`, `_match`, `_dispatch`, `_handle_error`.

**See also:** test `test_expected_control_flow_is_not_reported (tests/test_exception_tickets.py)`.

### `MailMailExceptionReport` [@ANCHOR: hams_helpdesk:hams_helpdesk_mail_exception_mail_mail_exception_report]

**Purpose and location.** Extends the Odoo model "mail.mail" with no new fields and the methods `_postprocess_sent_message`. Defined in `hams_helpdesk/models/mail_exception.py:57` as an Odoo model; bases: `models.Model`. (extends `mail.mail`.)

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "mail.mail"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_postprocess_sent_message`.

**See also:** test `test_failed_send_makes_one_ticket_with_no_addresses_or_content (tests/test_exception_tickets.py)`.

### `MailThreadBounceReport` [@ANCHOR: hams_helpdesk:hams_helpdesk_mail_exception_mail_thread_bounce_report]

**Purpose and location.** Extends the Odoo model "mail.thread" with no new fields and the methods `_routing_handle_bounce`. Defined in `hams_helpdesk/models/mail_exception.py:73` as an Odoo model; bases: `models.AbstractModel`. (extends `mail.thread`.) It is an abstract model: it has no table and is only mixed into others.

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "mail.thread"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_routing_handle_bounce`.

**See also:** test `test_bounce_processing_failure_is_reported_and_reraised (tests/test_exception_tickets.py)`.

### `MailMessageDefanged` [@ANCHOR: hams_helpdesk:hams_helpdesk_mail_message_defanged_mail_message_defanged]

**Purpose and location.** Extends the Odoo model "mail.message" with no new fields and the methods `_defanged_ticket_ids`, `_to_store`. Defined in `hams_helpdesk/models/mail_message_defanged.py:19` as an Odoo model; bases: `models.Model`. (extends `mail.message`.)

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "mail.message"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_defanged_ticket_ids`, `_to_store`.

**See also:** test `test_03_the_chatter_of_a_phishing_ticket_shows_no_live_link (tests/test_phishing_link_defang.py)`.

### `MailThread` [@ANCHOR: hams_helpdesk:hams_helpdesk_mail_thread_untrusted_mail_thread]

**Purpose and location.** Extends the Odoo model "mail.thread" with no new fields and the methods `message_parse`, `_with_verified_reply_headers`. Defined in `hams_helpdesk/models/mail_thread_untrusted.py:31` as an Odoo model; bases: `models.AbstractModel`. (extends `mail.thread`.) It is an abstract model: it has no table and is only mixed into others.

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "mail.thread"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `message_parse`, `_with_verified_reply_headers`.

**See also:** test `test_28b_a_made_up_in_reply_to_does_not_exempt_bulk_mail (tests/test_inbound_spam_filter.py)`.

### `RelayException` [@ANCHOR: hams_helpdesk:hams_helpdesk_relay_exception_relay_exception]

**Purpose and location.** Model description: Relay exception and crash reports. Defined in `hams_helpdesk/models/relay_exception.py:37` as an Odoo model; bases: `models.AbstractModel`. (model `hams_helpdesk.relay.exception`.) It is an abstract model: it has no table and is only mixed into others.

**Fields.** none declared by this class.

**Class attributes.** `_name = "hams_helpdesk.relay.exception"`; `_description = "Relay exception and crash reports"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_client_key`, `_rate_limited`, `_record`, `_submit`.

**See also:** test `test_a_failure_inside_the_beacon_is_not_reported_by_the_beacon (tests/test_browser_exceptions.py)`.

### `ResPartner` [@ANCHOR: hams_helpdesk:hams_helpdesk_res_partner_res_partner]

**Purpose and location.** Extends the Odoo model "res.partner" with the fields `callsign`, `helpdesk_ticket_ids` and the methods `_compute_callsign`. Defined in `hams_helpdesk/models/res_partner.py:8` as an Odoo model; bases: `models.Model`. (extends `res.partner`.)

| Field | Type | Meaning |
|---|---|---|
| `callsign` | `Char` | Callsign: Relevant amateur radio callsign. (compute=`"_compute_callsign"`; store=`True`; readonly=`False`) |
| `helpdesk_ticket_ids` | `One2many` | Helpdesk Tickets (comodel `hams_helpdesk.ticket`) |

**Class attributes.** `_inherit = "res.partner"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_compute_callsign`.

**See also:** test `test_staff_see_name_callsign_and_email_computed_from_the_member (tests/test_exception_members.py)`.

### `ShiftHandoffWizard` [@ANCHOR: hams_helpdesk:hams_helpdesk_shift_handoff_shift_handoff_wizard]

**Purpose and location.** Model description: Shift Handoff Wizard. Defined in `hams_helpdesk/models/shift_handoff.py:9` as an Odoo model; bases: `models.TransientModel`. (model `hams_helpdesk.shift_handoff`.) It is a transient (wizard) model: its rows are short-lived and removed by Odoo's vacuum.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `ticket_id` | `Many2one` | Ticket (comodel `hams_helpdesk.ticket`; required=`True`) |
| `old_user_id` | `Many2one` | Current Assignee (comodel `res.users`; readonly=`True`) |
| `new_user_id` | `Many2one` | Next Shift Assignee (comodel `res.users`; required=`True`) |
| `handoff_notes` | `Text` | Handoff Notes: Detailed context for the incoming operator. (required=`True`) |

**Class attributes.** `_name = "hams_helpdesk.shift_handoff"`; `_description = "Shift Handoff Wizard"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `action_confirm_handoff`.

**See also:** test `test_a_signed_in_member_is_linked_to_the_fingerprint_and_the_ticket (tests/test_exception_members.py)`.

### `HamsHelpdeskUntrustedMixin` [@ANCHOR: hams_helpdesk:hams_helpdesk_untrusted_mixin_hams_helpdesk_untrusted_mixin]

**Purpose and location.** Model description: Untrusted text filter for tickets. Defined in `hams_helpdesk/models/untrusted_mixin.py:37` as an Odoo model; bases: `models.AbstractModel`. (model `hams_helpdesk.untrusted.mixin`.) It is an abstract model: it has no table and is only mixed into others.

**Fields.** none declared by this class.

**Class attributes.** `_name = "hams_helpdesk.untrusted.mixin"`; `_description = "Untrusted text filter for tickets"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_untrusted_field_specs`, `_untrusted_filter_vals`, `_untrusted_summary`, `_untrusted_record`, `create`, `_untrusted_log_only`, `write`, `message_post`, `_untrusted_attachment_count`, `safe_view`.

**See also:** test `test_nobody_is_linked_for_a_logged_out_visitor_a_service_account_odoobot_or_an_archived_account (tests/test_exception_members.py)`.

### `HamsHelpdeskTicketSuspicion` [@ANCHOR: hams_helpdesk:hams_helpdesk_untrusted_mixin_hams_helpdesk_ticket_suspicion]

**Purpose and location.** Model description: What the untrusted-text filter removed or flagged on a ticket (staff evidence). Defined in `hams_helpdesk/models/untrusted_mixin.py:271` as an Odoo model; bases: `models.Model`. (model `hams_helpdesk.ticket.suspicion`; default order "id desc".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | no label or help in the source (compute=`"_compute_name"`) |
| `ticket_id` | `Many2one` | no label or help in the source (comodel `hams_helpdesk.ticket`; required=`True`; index=`True`; ondelete=`"cascade"`) |
| `source` | `Char` | no label or help in the source |
| `score` | `Integer` | no label or help in the source |
| `removed` | `Integer` | no label or help in the source |
| `findings_json` | `Text` | List of {vector, location, count, excerpt, weight}; excerpts are escaped. |
| `raw_original` | `Text` | The text exactly as received, kept for evidence. Staff only. Never given to an AI. |
| `findings_summary` | `Char` | no label or help in the source (compute=`"_compute_findings_summary"`) |

**Class attributes.** `_name = "hams_helpdesk.ticket.suspicion"`; `_description = "What the untrusted-text filter removed or flagged on a ticket (staff evidence)"`; `_order = "id desc"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_compute_name`, `_compute_findings_summary`.

**See also:** test `test_a_signed_in_member_is_linked_to_the_fingerprint_and_the_ticket (tests/test_exception_members.py)`.

### `Result` [@ANCHOR: hams_helpdesk:hams_helpdesk_untrusted_text_result]

**Purpose and location.** text: the clean text (or sanitized HTML). plain: its visible text. findings: list of dicts. Defined in `hams_helpdesk/models/untrusted_text.py:53` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`, `suspicious`, `__iter__`.

**See also:** test `test_26_a_failed_dmarc_spf_dkim_adds_weight_only (tests/test_inbound_spam_filter.py)`.

### `_Sanitizer` [@ANCHOR: hams_helpdesk:hams_helpdesk_untrusted_text_sanitizer]

**Purpose and location.** The class has no docstring; from its definition it provides `__init__`, `_note_drop`, `_attr_texts`, `handle_starttag`, `handle_startendtag`, `handle_endtag`, `handle_data`, `handle_comment`. Defined in `hams_helpdesk/models/untrusted_text.py:651` as a class; bases: `HTMLParser`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`, `_note_drop`, `_attr_texts`, `handle_starttag`, `handle_startendtag`, `handle_endtag`, `handle_data`, `handle_comment`, `unknown_decl`, `handle_pi`.

**See also:** test `test_01_sanitizer_methods_keep_allowed_tags_and_drop_everything_else (tests/test_untrusted_text_api.py)`.
