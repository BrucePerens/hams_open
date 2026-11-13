# Story: Helpdesk Ticket Creation

## Creating and routing a ticket [@ANCHOR: hams_helpdesk:COMM_helpdesk_ticket_creation]

Ticket creation is an intelligent process that automatically routes incidents to the appropriate on-duty personnel.

1.  **Incoming Trigger**: A ticket is created via UI, API, or automated integration -- including inbound support email, which the mail-ingest daemon hands off to `ingest_inbound_email()` over the JSON-2 API as a base64-encoded raw MIME message, restricted to the dedicated `mail_ingest_service_internal` account **[@ANCHOR: COMM_helpdesk_mail_ingest]**. An inbound email that `message_process()` resolves to an existing customer partner is linked to that customer's `partner_id` on the new ticket via `message_new()` **[@ANCHOR: COMM_helpdesk_message_new]**, the same way a portal- or backend-created ticket already is -- without it, an emailed-in ticket would never appear under that customer's own `/my/tickets`. A third trigger is GitHub itself: `/webhook/github` **[@ANCHOR: COMM_github_webhook_receive]** receives GitHub's own signed `dependabot_alert` and `workflow_run` webhook POSTs (HMAC-verified via `X-Hub-Signature-256`, matching `ses_webhook`'s own signature-gate-before-dispatch shape), and hands off to `_handle_dependabot_alert()` **[@ANCHOR: COMM_github_webhook_handle_dependabot_alert]** or `_handle_workflow_run()` **[@ANCHOR: COMM_github_webhook_handle_workflow_run]** depending on the event type -- a new, unactioned security alert or a CI failure becomes a ticket the same way any other trigger's ticket does, deduplicated by the alert's/run's own `html_url` via `_github_webhook_ticket_for()` **[@ANCHOR: COMM_github_webhook_ticket_for]** so a GitHub retry never opens a second ticket for the same event.
2.  **On-Duty Lookup**: The system queries the `pager_duty` adaptors to find the current "On-Duty" admin. ("On-Duty" is defined in the Architecture section of [the module README](../../README.md): without the `pager_duty` module, or when no shift is in force, nobody is on duty and the ticket stays unassigned.)
3.  **Automatic Assignment**: The `user_id` is set to the on-duty admin if unassigned.
4.  **Notifications**: Toast notifications and emails are sent to the assignee.
5.  **Pre-Shift Awareness**: Upcoming shift operators are CC'd on tickets created shortly before their shift. (The module README, under "Ticket Creation", notes that this step currently does nothing, because nothing in this repository supplies upcoming shifts.)
6.  **AI Triage Wake-Up**: after the transaction commits, a ticket created in the `new` stage drops a tiny spool file (`ticket-<id>.json`, the integer id and nothing else) for the AI triage daemon **[@ANCHOR: hams_helpdesk:COMM_triage_wakeup_on_create]**, written by `write_triage_wakeups()` **[@ANCHOR: hams_helpdesk:triage_wakeup_write]**. It is fire-and-forget: a failure is logged and never reaches the create. A ticket in the spam quarantine stage writes nothing. The daemon's own debounce, daily caps and kill switch decide what happens next (`daemons/ticket_triage_agent/README.md` in hams_com).

7.  **The Submitter's Callsign**: a ticket takes its callsign from its partner, so the partner's own callsign has to be right. The partner's callsign follows the callsign of the partner's login user and is recalculated whenever that changes, which is what lets a member's real, on-file callsign reach every ticket they submit through `/my/tickets/new`. A partner whose user has no callsign, or who has no login at all (a phone-in contact a support agent entered), keeps the value typed in by hand rather than having it blanked. The user is read through an internal facility service account, because this calculation can be triggered by a write to any partner by any caller, including narrowly scoped service accounts that cannot read users themselves **[@ANCHOR: hams_helpdesk:res_partner_compute_callsign]**.

**Interface:** `hams_helpdesk.ticket.create(vals_list)` (`@api.model_create_multi`, in `models/helpdesk_ticket.py`) takes a list of value dicts and returns the recordset in the same order. Before the insert it looks up the on-duty admin once (a failure is logged, the ticket stays unassigned), drops the triage verdict fields a caller may have sent, fills `website_id` from the context key `website_id`, `company_id` from that website, `callsign` from the partner, `user_id` from the on-duty admin, forces priority 3 for the child-safety type, and decides `notify_state` (`suppressed` for the spam stage, `held` while the triage review window is open, else `sent`). A batch that mixes quarantined and real tickets is created in two calls. After the insert it schedules the triage wake-up and runs `_automated_routing_and_notification()` under the helpdesk service account. **Errors:** a missing service account raises, by design. **Security:** no `sudo()`; elevation is by service account only.

## Verified by the creation and routing test [@ANCHOR: hams_helpdesk:COMM_test_01_ticket_creation_and_routing]

`test_01_ticket_creation_and_routing` (`tests/test_helpdesk_core.py`) creates a ticket for a portal member with the on-duty lookup and the bus patched, and asserts that the ticket goes to the on-duty manager (unassigned when `pager_duty` is not installed) and that the member's partner follows the ticket so the stage mail-back reaches them. A second test, `test_bug_dependency_crash`, proves creation does not raise when `pager_duty` is absent.

## Detailed Feature Index

The creation process uses the standard views below; each entry names the view, what it shows and how it is tested.

### The ticket form [@ANCHOR: hams_helpdesk:COMM_helpdesk_ticket_form]

View `hams_helpdesk.view_hams_helpdesk_ticket_form` (`views/helpdesk_ticket_views.xml`) is the staff form. The header holds the Close Ticket button (`action_close`), the Shift Handoff button (`action_shift_handoff`, hidden once Resolved or Closed), three administrator-only child-safety report buttons, and the `stage` statusbar (clickable). Above the sheet are the suspicion notice, the red phishing banner when links are defanged and the triage verdict notice. Staff only; a portal user never reaches it. Tested by `test_helpdesk_operator_rendering` and the operator tour.

### The ticket list [@ANCHOR: hams_helpdesk:COMM_helpdesk_ticket_list]

View `hams_helpdesk.view_hams_helpdesk_ticket_list` is the operator list of all tickets the user may read. Columns: id, subject, callsign, assignee, stage, priority, a suspicion flag (red when set) and, hidden by default and for staff only, the affected-member count and latest member. It backs the Helpdesk Tickets action (list then form) and the Spam / Phishing review queue (domain `stage = spam`). Tested by `test_helpdesk_operator_rendering` (`get_view` of the list) and the operator tour.

### The portal ticket list [@ANCHOR: hams_helpdesk:COMM_helpdesk_portal_list]

QWeb template `hams_helpdesk.portal_my_tickets`, rendered by the route `GET /my/tickets` (`portal_my_tickets` in `controllers/portal.py`, 20 tickets per page, newest first, login required). It shows a New Ticket button and a table of ticket number, title, callsign and status, or a "no tickets" note. The controller reads under the helpdesk service account but filters by hand to the member's own partner and to tickets of this website or of no website. Tested by `test_helpdesk_portal_rendering` and the portal tour.

### The portal ticket detail [@ANCHOR: hams_helpdesk:COMM_helpdesk_portal_detail]

QWeb template `hams_helpdesk.portal_ticket_detail`, rendered by `GET /my/ticket/<id>`. It shows the subject, status badge, callsign, priority, the description and the communication history (internal notes are excluded by the portal thread), and a Close Ticket form (CSRF (cross-site request forgery) token) while the ticket is not closed. A ticket that does not exist, belongs to another partner or to another website redirects to `/my`. Tested by `test_portal_ticket_detail_shows_own_ticket_and_denies_others` and the portal tour.

### The portal new-ticket form [@ANCHOR: hams_helpdesk:COMM_helpdesk_portal_new]

QWeb template `hams_helpdesk.portal_ticket_new`, rendered by `GET /my/tickets/new` and posted to `POST /my/tickets/submit` (CSRF protected). Fields: subject (required), callsign (pre-filled from the member's profile, optional), category (the model's ticket types minus the child-safety and automatic-exception types; an unknown `?ticket_type=` falls back to `general`) and description. A prospective ham without a callsign is not refused. Tested by `test_helpdesk_portal_rendering`, `test_portal_ticket_new_renders_without_callsign` and `test_portal_ticket_new_deep_link_preselects_a_real_category`.

## Amazon SES scan verdicts count as spam signals [@ANCHOR: hams_helpdesk:ses_verdicts]

Mail to the support addresses passes through Amazon SES (Amazon Simple Email Service), whose receipt rule has scanning on, so each stored message carries
`X-SES-Spam-Verdict` and `X-SES-Virus-Verdict`. `message_parse` keeps them and both inbound routes (admin@/support@ through
`hams_helpdesk.ticket.message_new`, info@/postmaster@ through `pager.incident.message_new`) treat a `FAIL` of either like
any other spam signal: the ticket starts in the Spam / Phishing stage with the reason in its first note, the same reversible
quarantine as the filter's own signals. `PASS`, `GRAY` and `PROCESSING_FAILED` are ignored. A DKIM, SPF or DMARC failure is not a
reason by itself (mailing lists and forwarders legitimately fail them); since 2026-11-03 each is one weak, weighted signal in the
bulk-mail rules below.

**Status:** Verified by `test_17_ses_verdicts_flag_only_a_fail`, `test_18_a_message_without_ses_headers_has_no_verdicts` and
`test_19_a_hostile_verdict_value_is_bounded` (tests/test_inbound_spam_filter.py).

## Bulk and marketing mail: weighted signals, one entry point

Production tickets 113 to 140 (health-supplement newsletters, "Your subscription renews soon", "Security alert for ...") were HTML
marketing mail that matched no subject pattern and arrived as `[PAGER] ...` tickets, because the pager route never saw the headers.
`mail_thread_untrusted.message_parse` now parks `mail_signals(message)` (header facts and the HTML shape of the raw MIME) beside the
SES verdicts, and both routes call the one function below. A flagged message still becomes a ticket in the visible Spam / Phishing
stage with every reason in its first note; nothing is deleted and a person can move it back to New.

### `inbound_spam_reasons` [@ANCHOR: hams_helpdesk:inbound_spam_reasons]

Purpose: decide whether one inbound message is quarantined, and say why in plain English. Pure Python, no ORM, safe on hostile input.
Defined in `hams_helpdesk/models/inbound_spam_filter.py`; called by `hams_helpdesk.ticket.message_new()` and
`pager.incident.message_new()`.

| Argument | Type | Meaning |
|---|---|---|
| `subject` | str or None | The message subject. `None` reads as empty. |
| `body_html` | str or None | The Odoo-sanitised HTML body of the message (`msg_dict["body"]`). Images, the plain-text part and all headers are already gone from it. |
| `signals` | dict or None | `mail_signals()` of the raw message; `None` or `{}` when only the stored subject and body exist. |
| `verdicts` | dict or None | `ses_verdicts()` of the raw message: `{"spam": "FAIL", "virus": "PASS"}`. |
| `trusted_sender` | bool | True when the From address resolved to a partner with a user account (a member or staff). |

Returns a list of reason strings, empty when the message is not flagged. Raises nothing for any input.

Order of evaluation: (1) a sender whose From domain is one of `OWN_SENDER_DOMAINS` and for whom SES recorded a DKIM or DMARC pass is
never flagged (our monitoring and the inbound path check; a forged From fails authentication and is judged normally); (2) a
`trusted_sender` is never flagged unless SES recorded a DMARC failure (a spoofed member address is judged); (3) the strong rules, each
enough alone: subject patterns, brand-impersonation links, SES spam or virus `FAIL`; (4) the weighted bulk rules.

Weighted bulk rules (`bulk_mail_findings`, weights in parentheses; the message is flagged when the total reaches
`BULK_SCORE_THRESHOLD`, 5, so at least three facts are needed): `List-Unsubscribe` header (2); `Precedence: bulk|list|junk` (2);
`List-Id` (1); `Auto-Submitted` other than `no` (1); SES DMARC failure (2), SPF failure (1), DKIM failure (1); Return-Path
organisational domain differs from the From domain (1); HTML with no plain-text alternative, or a plain part under 200 characters
against more than 3000 characters of HTML (1); four or more tables or nesting three deep (1); three or more images (1); a 0 or 1
pixel image (1); "unsubscribe" in the body, counted only when there is no `List-Unsubscribe` header because they are one fact (2);
"view in browser" footer (1); "update your preferences" or "you are receiving this because" footer (1); a health-supplement,
"secret", "researchers" or "natural" clickbait subject (1, counted once). A reply (a message whose `In-Reply-To` or `References`
names a Message-ID already stored on a ticket or incident; the headers alone are the sender's claim) is never judged by the weighted rules. Only the authentication results written by Amazon SES itself (the first
`Authentication-Results` header whose authserv-id is `amazonses.com`) are read; any other copy is the sender's claim.

Special concerns: the first reason of a weighted flag states the total and the threshold, the following lines one signal each, so a
reviewer can see why. `STORED_TICKET_SCORE_THRESHOLD` (4) is the lower threshold used only by the one-off pass over tickets already
stored (hams_com `devbox_tools/reclassify_bulk_spam_tickets_hams_prod.py`), which has neither headers nor images; its dry run is read
by a person before anything changes. False positives are worse than false negatives: a flagged message is only moved to a visible
stage.

**Status:** Verified by `TestBulkMailSignals` (tests/test_inbound_spam_filter.py), which runs the real ticket names of hams_prod.

### `referenced_message_ids` [@ANCHOR: hams_helpdesk:inbound_referenced_message_ids]

Purpose: list the Message-IDs a raw inbound message says it replies to, so the caller can check them against stored mail. Pure (no
database); called by `mail_signals()`.

| Argument | Type | Meaning |
|---|---|---|
| `message` | `email.message.Message` | The parsed raw message; only the `In-Reply-To` and `References` headers are read (at most 20,000 characters of each). |

Returns a list of `<local@host>` strings in header order without repeats, at most `MAX_REFERENCED_IDS` (10, the last ones, which are the
nearest ancestors); `[]` when the headers are absent or hold no `<...>` token. Raises nothing for any header text.

**Status:** Verified by `test_28b_a_made_up_in_reply_to_does_not_exempt_bulk_mail` (tests/test_inbound_spam_filter.py).

### `REPLY_MESSAGE_MODELS` [@ANCHOR: hams_helpdesk:inbound_reply_message_models]

A module constant in `models/mail_thread_untrusted.py`: the list `["hams_helpdesk.ticket", "pager.incident"]` of Odoo model names a stored
`mail.message` may belong to for its Message-ID to count as a real thread (an inbound message becomes a ticket, or a pager incident
through the pager adapter). Read only, by `_with_verified_reply_headers()` as the `model` term of its lookup; a model that is not
installed simply matches nothing. Immutable by convention; add a model here when another record type starts receiving inbound mail.

### `_with_verified_reply_headers` [@ANCHOR: hams_helpdesk:inbound_verified_reply_headers]

Purpose: decide whether a message really is a reply, because `detect_bulk_mail_signals()` skips the bulk rules for a reply and the
sender writes `In-Reply-To` and `References` (a bulk sender could add any value and escape the weighted rules). Called by
`mail.thread.message_parse()` (`models/mail_thread_untrusted.py`) on the `mail_signals()` dict before it is parked.

| Argument | Type | Meaning |
|---|---|---|
| `signals` | dict | A `mail_signals()` dict; mutated (the `reply_headers` key is set) and returned. |

Returns the same dict with `reply_headers` True only when one of `signals["referenced_message_ids"]` is the `message_id` of a
`mail.message` on a `hams_helpdesk.ticket` or a `pager.incident` (`REPLY_MESSAGE_MODELS`; the service account sees only the records it can read, so an unreadable thread counts as not found and the message is judged); False otherwise, including when the list is empty. One `search_count` per message, run as
the helpdesk service account (`hams_helpdesk.user_helpdesk_service`) because the mail-ingest identity cannot read tickets. No other
side effect; a database error propagates to the mail ingest, which retries the message.

**Status:** Verified by `test_28b_a_made_up_in_reply_to_does_not_exempt_bulk_mail`.

### `mail_signals` [@ANCHOR: hams_helpdesk:inbound_mail_signals]

Purpose: read the header facts and HTML shape of one raw inbound message, once, in `mail.thread.message_parse()`, because Odoo's
sanitised ticket body has lost all of them. The dict is parked on the cursor beside `ses_verdicts` (cleared on commit and rollback)
and read by `helpdesk_ticket.message_new()` and `pager.incident.message_new()`.

| Argument | Type | Meaning |
|---|---|---|
| `message` | `email.message.Message` | The parsed raw message, headers and all parts. |

Returns a dict; a key is absent when the fact is unknown. Never raises: an unreadable part contributes nothing and the error class is
logged at info level. It reads at most 400,000 characters of each text part and ignores attachments.

| Key | Type | Meaning |
|---|---|---|
| `list_unsubscribe`, `list_id` | bool | The header is present. |
| `referenced_message_ids` | list of str | The Message-IDs (`<local@host>`) named by `In-Reply-To` and `References`, in header order, at most 10 (the last ones). The sender writes them, so they are only a claim. |
| `reply_headers` | bool | **Not set by `mail_signals`.** `message_parse()` sets it through `_with_verified_reply_headers()` (below): true only when one of `referenced_message_ids` is the Message-ID of a message already stored on a ticket. |
| `precedence` | str | Lower-cased `Precedence` value, at most 16 characters. |
| `auto_submitted` | str | Lower-cased `Auto-Submitted` value, at most 32 characters. |
| `from_domain`, `return_path_domain` | str | Lower-cased domain of the first address of `From` and `Return-Path`, at most 255 characters; "" when absent. |
| `spf`, `dkim`, `dmarc` | str | `pass`, `fail`, `none`, ... from the first `Authentication-Results` header whose authserv-id is `amazonses.com`; absent when SES wrote none. |
| `has_plain` | bool | A `text/plain` part exists. |
| `plain_chars`, `html_chars` | int | Stripped length of the plain parts and length of the HTML parts. |
| `tables`, `table_depth`, `images`, `tracking_pixels` | int | `html_shape()` of the HTML parts, summed (depth is the maximum). |

No locks, threads or network. The sender controls every header except the SES `Authentication-Results` and `X-SES-*` ones, so each
fact is only ever a weak signal.

**Status:** Verified by `test_26_a_failed_dmarc_spf_dkim_adds_weight_only` and `test_27_only_the_ses_authentication_results_header_is_believed`.

### `html_shape` [@ANCHOR: hams_helpdesk:inbound_html_shape]

`html_shape(html)`: `html` is a str (or None, read as empty). Returns `{"tables": int, "table_depth": int, "images": int,
"tracking_pixels": int}`: the number of `<table>` tags, the deepest nesting, the number of `<img>` tags, and the number of images
whose `width` or `height` attribute is 0 or 1 (with or without `px`). Counted from tags with a regular expression over the first
400,000 characters, so unbalanced or hostile markup cannot raise and a stray `</table>` never makes the depth negative. Pure,
no side effects. **Status:** Verified by `test_30_hostile_input_never_raises`.

### `own_authenticated_sender` [@ANCHOR: hams_helpdesk:inbound_own_authenticated_sender]

`own_authenticated_sender(signals)`: `signals` is a `mail_signals()` dict (or None). Returns True only when `from_domain` is one of
`OWN_SENDER_DOMAINS` (`hams.com`) or a sub-domain of it AND SES recorded a `dkim` or `dmarc` pass. A forged From line fails
authentication and returns False; `evil-hams.com` is not a sub-domain. Pure. `inbound_spam_reasons()` returns [] for such a sender.
**Status:** Verified by `test_28_replies_members_and_our_own_authenticated_senders_are_exempt`.

### `bulk_mail_findings` [@ANCHOR: hams_helpdesk:inbound_bulk_mail_findings]

`bulk_mail_findings(subject, body_html, signals=None)`: `subject`, `body_html` as for `inbound_spam_reasons`; `signals` the
`mail_signals()` dict, or None when only stored text exists (then the header and plain-text rules cannot fire and the shape is read
from `body_html`). Returns a list of `(weight, reason)`, every weak signal present with its integer weight (1 or 2) and a
plain-English reason; the caller applies the threshold. Pure, never raises. The weights are listed under `inbound_spam_reasons`.
**Status:** Verified by `test_25_each_weak_signal_has_its_own_reason_and_weight`.

### `detect_bulk_mail_signals` [@ANCHOR: hams_helpdesk:inbound_detect_bulk_mail_signals]

`detect_bulk_mail_signals(subject, body_html, signals=None, threshold=BULK_SCORE_THRESHOLD)`: arguments as above; `threshold` is the
total weight needed (live mail 5, `STORED_TICKET_SCORE_THRESHOLD` 4 for the one-off pass over stored tickets). Returns [] below the
threshold or when `signals["reply_headers"]` is true; otherwise a first reason stating the total and the threshold followed by one
`bulk signal (+N): ...` reason per finding. Pure, never raises. **Status:** Verified by
`test_21_the_real_missed_newsletters_are_flagged_from_the_stored_ticket_alone`.

### `inbound_sender_is_member` [@ANCHOR: hams_helpdesk:inbound_sender_is_member]

`hams_helpdesk.ticket._inbound_sender_is_member(author_id)` (model method). `author_id`: int res.partner id the mail gateway matched
to the From address, or `False`/`None`. Returns True when that partner has at least one active user; False otherwise, including for
a partner that is only a contact. Runs as the helpdesk service account because the mail-ingest identity cannot read `res.users`.
No side effects. Mail from a member is never run through the spam filter (but see the DMARC exception above).

### A quarantined ticket mails nobody [@ANCHOR: hams_helpdesk:COMM_helpdesk_spam_create_sends_no_mail]

`create()` switches off Odoo's own "you have been assigned" email (the `mail_auto_subscribe_no_notify` context key) when every ticket
of the call is created in the `spam` stage. A call that mixes spam and real tickets is split in two so the real ones keep their
assignment email; the returned recordset keeps the order of `vals_list`. Together with the gates in
`_automated_routing_and_notification` (custom assignment notice, toast, and the "Shift CC" note to the next shift's admins), a
ticket in the spam stage sends no email, notification or bus message to anyone. On the pager route the incident itself is created
already resolved and without a board refresh (see the pager_duty incident lifecycle).

## Convention hotel-booking offers to exhibitors [@ANCHOR: hams_helpdesk:inbound_housing_solicitation]

**Purpose.** Third-party "housing" companies mail exhibitors of a convention (found live 2026-11-03: ticket 141, a
"Hotel Booking Team" message "Dear Exhibitor, Hotel options for HamCation 2027 are now available ... View Hotel
Options", sent from a bulk-mail service to the admin@ alias). `detect_inbound_spam_signals` flags such a message as
spam so it goes to the visible spam stage and notifies nobody. Bruce confirmed the ticket was spam; the filter had
found nothing in it because it names no brand and carries no known lure subject.

**Data.** Two compiled patterns in `hams_helpdesk/models/inbound_spam_filter.py`:

| Name | Type | Matches |
|---|---|---|
| `HOUSING_SALUTATION_RE` | compiled regular expression, case-insensitive | a greeting to exhibitors, vendors, attendees, sponsors or booth holders ("Dear Exhibitor") |
| `HOUSING_OFFER_RE` | compiled regular expression, case-insensitive, dot matches newline | the words "hotel options", "hotel rates", "hotel booking", "hotel reservation(s)" or "hotel accommodation(s)" followed within 200 characters by "available", "reserve" or "book" |

**Interface.** Used inside `detect_inbound_spam_signals(subject, body_html)` (unchanged signature; returns a list of
plain-English reason strings, empty when nothing matched, never raises). The HTML tags are removed from `body_html`
first. When BOTH patterns match the body, one reason is added: "body is an unsolicited convention hotel-booking offer
to exhibitors (a third-party housing solicitation)". It is a strong signal: on its own it makes
`inbound_spam_reasons` non-empty.

**Special concerns.** Synchronous, pure, no database or network access; the patterns are bounded (a 200 character
window), so a hostile message cannot make them run long. Both halves are required so that a member asking about hotel
rates near a hamfest, or an organizer writing "Dear Exhibitor, your booth number is 12", is never flagged. The
organizers' own mail is not whitelisted here: if a real organizer message is ever misjudged the recovery is the
visible spam stage (a person moves the ticket back to new), and a sender allow-list is the next step.

**Tests.** `test_31_a_convention_hotel_booking_offer_to_exhibitors_is_flagged` (tests/test_inbound_spam_filter.py).

