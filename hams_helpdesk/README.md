# Hams Helpdesk

*Copyright © Bruce Perens K6BP.*
SPDX-License-Identifier: AGPL-3.0-or-later

Zero-Sudo compliant, lightweight helpdesk management designed for deep SRE (Site Reliability Engineering) integration.

<system_role>
Zero-Sudo compliant, lightweight helpdesk management system designed for SRE (Site Reliability Engineering) workflows. It prioritizes traceability, minimal privilege, and seamless integration with calendar-based on-duty rotations.
</system_role>

<architecture>
The module implements a reactive ticketing system where assignment is driven by on-duty status. It uses a wizard-based handoff mechanism to ensure context is preserved during operator shifts.

"On-duty" means the user that `calendar.event.get_current_on_duty_admin()` returns. This module's own version of that method returns nobody; the `pager_duty` module overrides it to return the user of the pager-duty calendar shift in force right now for the current website (a shift with no website counts for every website; if several overlap, the most recently created wins). This module does not depend on `pager_duty`, so without it, or when no shift is in force, new tickets are left unassigned. A failure in that lookup is logged and the ticket is still created, unassigned.

- **Models**:
    - `hams_helpdesk.ticket`: Main ticket entity, inherits `mail.thread` for communication.
    - `hams_helpdesk.shift_handoff`: Transient wizard for secure ownership transfer.
- **Other ticket sources** (besides the backend form and the portal):
    - Inbound email to the `admin` and `support` mail aliases becomes a ticket; `message_new()` links it to the sender's existing contact as the Customer. `ingest_inbound_email()` is the entry point for the inbound-mail polling daemon and refuses any caller except the `mail_ingest_service_internal` account. It returns `{"status": "ingested"}`, or `{"status": "unroutable", ...}` for mail addressed to no alias (a deterministic miss, so the daemon moves the object to `failed/` without retrying instead of getting a 500).
    - `POST /webhook/github` accepts GitHub `dependabot_alert` (created or reopened) and `workflow_run` (completed with a failure) events whose `X-Hub-Signature-256` HMAC matches the `hams_helpdesk.github_webhook_secret` system parameter, and opens one ticket per alert or run URL. A repeat delivery for a URL whose ticket is still open adds nothing; once that ticket is Resolved or Closed, a new delivery opens a new ticket. With no secret configured, every request is refused with 403.
    - The `pager_duty` module opens tickets for its incidents (see documentation.html).
- **Child-safety mandatory reports**: the ticket type `csam_enticement_trafficking` drives the NCMEC CyberTipline reporting workflow (18 U.S.C. Sec. 2258A). Such a ticket is forced to Critical priority, every Helpdesk Manager is notified, and a report packet is assembled on the ticket. Portal users cannot choose this type, and the external AI triage account's record rule does not include it, so that account cannot read it. See documentation.html.
- **External AI triage**: the `ai_triage_service_internal` account has read-only access to tickets of type `general` and `hams_local_relay` only (and never to one the untrusted-text filter marked suspicious), and posts its analysis through `mcp_post_internal_note()` as an internal note the customer does not see.
- **Security**: Strict record rules (Odoo `ir.rule`) enforce that portal users only see their own tickets, helpdesk users see tickets assigned to them or unassigned, and helpdesk managers see every ticket. All three are limited to the user's own companies.
- **Multi-Website**: Supports multiple websites by segregating tickets via `website_id`.
- **Multi-Company**: Full multi-company support with strict record rules and automatic company inference: a ticket created with a website but no explicit company takes that website's company.
</architecture>

<security_design>
- **Zero-Sudo Compliance**: No `.sudo()` calls are allowed. All privilege elevations must use service accounts (`hams_helpdesk.user_helpdesk_service` or `zero_sudo.odoo_facility_service_internal`) via `zero_sudo`. The one exception is `ingest_inbound_email()`, which calls `message_process()` with `.sudo()` under an explicit `burn-ignore-sudo` tag, because `message_process()` elevates internally on any alias match anyway. (`pager_duty.user_pager_service_internal` is used by the `pager_duty` module, not by this one.)
- **Micro-Privilege**: Uses `res.groups.privilege` to define granular access.
- **Portal Isolation**: Portal users are strictly limited via record rules to their own `partner_id` and authorized `website_id`.
- **Fail-Fast Integrity**: The module is designed to fail fast if required service accounts are missing or misconfigured, preventing silent failures and ensuring operational reliability. Concretely, ticket creation raises an error if `hams_helpdesk.user_helpdesk_service` cannot be resolved, while a failure looking up the on-duty or upcoming-shift users is only logged.
</security_design>

## Technical Architecture & Anchors

This module operates within strict DevSecOps parameters, ensuring all actions are traceable and privileged escalations are explicitly avoided.

* **Ticket Lifecycle (`[@ANCHOR: helpdesk_ticket_lifecycle]`)**: Defines the stages and constraints of an issue, natively integrating with mail threads.

    - Verified by [@ANCHOR: test_01_ticket_creation_and_routing]

* **Hidden-text colour checks (`[@ANCHOR: helpdesk_untrusted_css_number]`)**: a CSS number in attacker-written style or colour text is read as a finite float or as nothing, so the hidden-text detector never raises on `nan`, `inf` or a 400-digit value.
* **Ticket Creation (`[@ANCHOR: helpdesk_ticket_creation]`)**: Intercepts the ORM `create` method to automatically execute pre-shift CC logic and route to the currently on-duty personnel based on calendar availability. "Pre-shift CC" subscribes the users returned by `calendar.event.get_upcoming_duty_shifts()` (people whose shift starts soon) and sends them an internal "Shift CC" note. This module's own version of that method returns no shifts and no module in this repository overrides it, so the step currently does nothing.

    - Verified by [@ANCHOR: test_01_ticket_creation_and_routing]

* **Shift Handoff Initiation (`[@ANCHOR: helpdesk_shift_handoff]`)**: UI action triggering the secure transfer wizard, ensuring the leaving operator leaves context.

    - Verified by [@ANCHOR: test_02_shift_handoff_wizard]

* **Handoff Execution (`[@ANCHOR: helpdesk_handoff_execution]`)**: The backend transaction that officially modifies ownership and commits the transfer briefing to the unalterable chatter log, as an internal note that the customer does not see.

    - Verified by [@ANCHOR: test_02_shift_handoff_wizard]

* **Documentation Injection (`[@ANCHOR: helpdesk_doc_injection]`)**: Automated bootstrapping of user documentation into the central knowledge base via the `zero-sudo` documentation facility, which loads the pages listed under `knowledge_docs` in this module's `__manifest__.py`.

    - Verified by [@ANCHOR: test_05_doc_injection]

* **Multi-Website & Multi-Company Awareness (`[@ANCHOR: helpdesk_multi_website]`)**: Tickets are associated with specific websites and companies to ensure proper data isolation in multi-tenant environments.

    - Verified by [@ANCHOR: test_06_multi_website_awareness_logic]

* **Micro-Privilege Security (`[@ANCHOR: helpdesk_micro_privilege]`)**: Access is strictly controlled via record rules and explicit field-level security in the ORM: `write()` refuses a portal user's change to stage, assignee, priority, customer, ticket type, website, company, calendar event, the archive flag, or any `ncmec_*` field.

    - Verified by [@ANCHOR: test_05_portal_write_restrictions]
* **Helpdesk Operator Workflow**: Verification of the backend operator's ability to manage tickets and execute handoffs.
    - Verified by [@ANCHOR: helpdesk_operator_tour]
* **Helpdesk Portal Facility**: Verification of the portal customer's ability to submit and view tickets.
    - Verified by [@ANCHOR: helpdesk_portal_tour]

* **Portal Ticket Closure**: Customer ability to close their own tickets. ([@ANCHOR: helpdesk_portal_close])

    - Verified by [@ANCHOR: test_portal_close_ticket]

## Stories and Journeys

### Ticket Lifecycle Management ([@ANCHOR: helpdesk_ticket_lifecycle])
**Goal**: Efficiently track and resolve system issues while maintaining a clear audit trail.
1.  **Incoming Request**: A new ticket is created, either manually or via automated incident detection.
2.  **Automated Routing**: The system identifies the currently on-duty administrator (`[@ANCHOR: helpdesk_ticket_creation]`) and assigns the ticket.

3.  **Progression**: The operator manages tickets via the list ([@ANCHOR: helpdesk_ticket_list]) and form ([@ANCHOR: helpdesk_ticket_form]) views, moving through stages: New -> In Progress -> Resolved -> Closed. A fifth stage, Spam / Phishing (Quarantined), is a side lane for flagged inbound mail, not a normal step (see documentation.html).
4.  **Customer Communication**: Every stage change except a move into Spam / Phishing triggers an automated update to the reporter (the ticket's Customer), if the ticket has one.

### Incident Resolution Journey ([@ANCHOR: journey_incident_resolution])
**Goal**: Complete the lifecycle of a critical incident from detection to resolution.
1.  **Detection**: System monitor detects a service failure.
2.  **Creation**: A Helpdesk ticket is created.
3.  **Assignment**: The system auto-assigns the ticket to the SRE currently "On-Duty".
4.  **Investigation**: SRE updates stage to "In Progress".
5.  **Resolution**: SRE fixes the issue, updates stage to "Resolved".
6.  **Closure**: After verification, the ticket is moved to "Closed".

### Shift Handoff Protocol ([@ANCHOR: journey_shift_handoff])
**Goal**: Ensure seamless continuity of operations when operators rotate shifts.
1.  **Initiation**: The outgoing operator selects "Shift Handoff" on an active ticket (`[@ANCHOR: helpdesk_shift_handoff]`).
2.  **Context Capture**: A wizard appears requiring the selection of the next assignee and detailed handoff notes. The Shift Handoff button is hidden once a ticket is Resolved or Closed.
3.  **Execution**: Upon confirmation, the system atomically updates ownership and logs a briefing (`[@ANCHOR: helpdesk_handoff_execution]`).

4.  **Verification**: The handoff is recorded in the chatter and the operator workflow is validated by [@ANCHOR: helpdesk_operator_tour].

### Portal Self-Service ([@ANCHOR: journey_portal_self_service])
**Goal**: Empower customers to manage their own tickets.
1.  **Submission**: Customer creates a ticket via the portal ([@ANCHOR: helpdesk_portal_new]).

2.  **Review**: Customer views their tickets in the list ([@ANCHOR: helpdesk_portal_list]) and detail ([@ANCHOR: helpdesk_portal_detail]) views.

3.  **Closure**: Customer closes the ticket when resolved ([@ANCHOR: helpdesk_portal_close]).

4.  **Verification**: Validated by [@ANCHOR: helpdesk_portal_tour].

## 🧪 Specialized Test Environment

This repository uses a specialized test environment with the following characteristics:
- **PostgreSQL Socket**: The database cluster listens on a Unix socket located at `/opt/hams/pgsock`.
- **Test Runner Flags**: Run `tools/test.py -u hams_helpdesk` to test this module. `tools/test.py` has no `--already-provisioned` option; its argument parser rejects unknown options.
- **Python Execution**: Use `/usr/bin/python3` to ensure access to system-installed Odoo dependencies.
- **Linter Overrides**: Use a custom ignore file (`-c <file>`) to bypass fragile tours in other modules if they block testing of this module.

## External Dependencies

None.

## Security & Multi-tenant Isolation
Hams Helpdesk implements strict isolation:
- Record rules ensure users only see tickets for their company.
- Website-level filtering ensures portal users only see tickets created on that specific website, plus tickets not tied to any website (for example, ones created from email).
- Controllers strictly validate ownership and website context before rendering ticket details.

## Automatic server-exception tickets
Unhandled exceptions from pages, RPC/ORM calls and scheduled jobs become tickets of the staff-only
category "Automatic: server exception", one per fingerprint (exception type, innermost addon frame,
module, normalized route), counted on `hams_helpdesk.exception.fingerprint` and listed under
Helpdesk > Server Exceptions. See `docs/stories/server_exception_tickets.md`.
For page, RPC and browser reports where the person was signed in, the ticket lists the member (name, callsign if any, email,
computed live from their account; staff only, at most 25 members per failure, the rest only counted) so the site team can write
to them: a link to the account, never a copy (section 17 of that story). A module that knows about guardianship can
substitute the supervising guardian, or no one; this module alone always lists the member. Scheduled jobs, mail and
external daemons have no member. A minor's
guardian is shown instead of the minor, through the `_contact_user` hook (`ham_communications_consent` in hams_com).

### Browser JavaScript errors

Uncaught JavaScript errors on website, portal and back-office pages are posted by
`static/src/js/browser_error_reporter.js` to `POST /hams_helpdesk/browser_exception` and become "Browser"
exception tickets through the same fingerprint pipeline. Not loaded by the relay-served console (a future
opt-in privacy decision). The browser sends its session cookie; the server links the signed-in member's account (a logged-out visitor stays anonymous). What is sent and stored, the checks and the reasons for them (no nonce; a current
asset bundle version instead): `docs/stories/server_exception_tickets.md`, section 16.

Production follow-ups, configured on hams1 and Cloudflare, not in this module (edge limits are the first
layer; the in-code Redis limit of 10 a minute and 60 an hour per visitor is the second):

- Cloudflare: a rate-limiting rule on `http.request.uri.path eq "/hams_helpdesk/browser_exception"` and method POST, counted per
  client IP, for example 20 requests per minute, action Block for 10 minutes.
- `nginx/prod/hams-tunnel-origin.conf` (not edited here): inside the catch-all server, a
  `limit_req_zone $http_cf_connecting_ip zone=hams_browser_exception:10m rate=10r/m;` (in the `http` context) and
  a `location = /hams_helpdesk/browser_exception { client_max_body_size 16k; limit_req zone=hams_browser_exception burst=10 nodelay; limit_req_status 429; <the catch-all's proxy settings> }`.
  The 16k cap matters: the application checks the declared size of a JSON body, but Odoo parses a form-encoded body
  before the controller runs.
- After deploying: `odoo -u hams_helpdesk` on hams1 (a new selection value, "Browser", and a new dependency on
  `distributed_redis_cache`), then `systemctl restart odoo`; the asset bundles regenerate by themselves.

### Relay exceptions and crashes

A `hams_local_relay` that panics, hits a fatal signal or reports an unexpected error sends a report on its authenticated uplink;
`ham_relay_bridge` (hams_com) finds the relay's owner and the acting operator and calls `hams_helpdesk.relay.exception._submit()`.
The tickets are titled "Relay exception: ..." or, for a fatal signal, "Relay crash: ...", one per stable fingerprint, with the
owner and operator linked as Reporting members. Wire form, limits, every function and the decisions:
`docs/stories/relay_exception_reports.md`. After deploying: `odoo -u hams_helpdesk`, then restart Odoo.

## Phishing links are defanged for staff

Bruce, 2026-11-08. A ticket whose triage verdict is `phishing` (the AI triage agent's verdict; see `docs/stories/triage_verdict_and_override.md`)
shows the links in its text as plain text to staff, with a warning banner, in three places: the ticket form, the chatter, and the
notification email. Tickets with any other verdict (`spam`, `real`, `unsure`, none) keep clickable links. A person who moves a ticket out
of the Spam stage overrides the verdict (`triage_overridden`), and the links are live again. The shared text filter (`untrusted_text.py`)
is not changed: it keeps `href` on purpose and its tests assert it. Nothing is stored in defanged form; the stored description and the
stored messages keep their links, and the form is chosen each time the text is shown, so a changed verdict or an override takes effect at
once. Story: `docs/stories/phishing_links_defanged.md`. Tests: `tests/test_phishing_link_defang.py`.

What the staff see: `<a href="https://x.example/a">Verify</a>` becomes the text `Verify (hxxps://x[.]example/a)`. `hxxp` and
`hxxps` stand for `http` and `https`, `[.]` for a dot and `[@]` for an at sign, so no browser, mail program or chat client makes it a
link, and a person can still read where it pointed. A URL written out in the text is defanged the same way; `src`, `action` and the other
URL attributes of non-link elements are removed.

Not covered: the portal view the ticket's own customer sees (a phishing sender reading their own ticket), and emails that do not carry the
ticket's message text (the assignment notice, the digest, the pager alert). They show no ticket text.

### defang_warning_text [@ANCHOR: hams_helpdesk:defang_warning_text]

The warning shown above a defanged ticket's text in the notification email (`models/link_defang.py`, constant `DEFANG_WARNING_TEXT`, a `str`).
The form banner in `views/helpdesk_ticket_views.xml` carries the same words (a test compares the key phrases). Written to the ADR-0112
wording rules: it says what is shown and what to do and promises nothing about privacy.

| Part | Meaning |
|---|---|
| "Phishing warning" | Opens the text; the form shows it in bold. |
| "hxxp and hxxps stand for http and https, and [.] stands for a dot" | Teaches the reader the defanged form. |
| "Do not open them" | The instruction. |
| "move it out of the Spam stage and its links work again" | How a person overrides a wrong verdict. |

### defang_url_text [@ANCHOR: hams_helpdesk:defang_url_text]

Purpose: turns one URL (or any string) into inert plain text. Called by `defang_html()` for every address it writes out.

| Argument | Type | Meaning |
|---|---|---|
| `url` | `str` or falsy | The address. Leading and trailing white space is dropped. Not checked to be a URL. |

Returns a `str`: the scheme `http` becomes `hxxp` (`https` becomes `hxxps`) and `ftp` becomes `fxp` (case is kept for the rest), every `.`
becomes `[.]`, every `@` becomes `[@]`. A falsy `url` gives `""`. Raises nothing. Pure, no side effects, safe to call from any thread.
Example: `defang_url_text("https://a.example.com/x")` is `hxxps://a[.]example[.]com/x`. See also `defang_html`; tested by
`test_01_a_url_becomes_inert_text`.

### defang_html [@ANCHOR: hams_helpdesk:defang_html]

Purpose: the one function that makes a text safe to read next to a phish. Called by the staff form's computed description, by the
chatter override and by the notification email override below.

| Argument | Type | Meaning |
|---|---|---|
| `value` | `str`, `Markup`, or falsy | An HTML fragment or plain text. Borrowed, never modified. |

Returns `Markup`, never `None`. A falsy `value` returns `Markup("")`. Order of work: (1) URLs written out in text nodes (`http://`,
`https://`, `ftp://`, `mailto:`, `www.`) are defanged; (2) each `<a>` or `<area>` becomes `<span class="o_link_defanged">` keeping its own
content and, when its text does not already show it, followed by the defanged address in parentheses; (3) every other element loses
`href`, `src`, `action`, `formaction`, `background`, `poster`, `srcset`, `data`, `cite`, `ping`, `xlink:href`. Other markup (bold, lists,
paragraphs) is kept; the text is not otherwise sanitised, because it already went through the shared filter when it was stored.
Errors: none raised. If the fragment cannot be parsed (a control character, for example) the whole text is escaped and shown as text,
with URLs defanged, which has no link either. Pure, one parse of the fragment, no database access. Tested by `test_02` to `test_06` of
`TestPhishingLinkDefangUnits`.

### ticket_links_defanged_fields [@ANCHOR: hams_helpdesk:ticket_links_defanged_fields]

Two computed, non-stored fields on `hams_helpdesk.ticket` (`models/helpdesk_ticket.py`). Nobody sets them; they are recomputed from the
verdict and the description each time they are read, so they cannot go stale.

| Field | Type | Meaning |
|---|---|---|
| `links_defanged` | Boolean | True when the ticket's links are shown as text (`_triage_links_defanged()`). Drives the banner and which description field the form shows. Depends on `triage_verdict`, `triage_overridden`. |
| `description_defanged` | Html (not sanitised again; read only) | `defang_html(description)` when `links_defanged`, otherwise the description itself. Depends on `description`, `triage_verdict`, `triage_overridden`. |

Invariant: `description_defanged` contains no `href` when `links_defanged` is true. Concurrency: none; computed per read in the caller's
transaction. Functions: `_compute_links_defanged`, `_compute_description_defanged`.

### triage_links_defanged [@ANCHOR: hams_helpdesk:triage_links_defanged]

Purpose: the single rule for whether a ticket shows its links as text. `_triage_links_defanged()` on one ticket returns True when
`triage_verdict == "phishing"` and `triage_overridden` is false. No argument. Returns `bool`. A recordset that is not exactly one ticket
(empty or several) returns False. Reads two fields; no side effects. The verdict stays `phishing` after a person's override (the pair is
the labelled example, see `triage_record_override`), which is why the override flag, not the verdict alone, decides. Tested by
`TestPhishingLinkDefangTickets` `test_01`, `test_02`, `test_05`, `test_06`, `test_07`.

### compute_links_defanged [@ANCHOR: hams_helpdesk:compute_links_defanged]

`_compute_links_defanged(self)`: the compute method of `links_defanged`; sets it to `_triage_links_defanged()` for each ticket in `self`.
No other arguments or return value, no side effects, cannot fail.

### compute_description_defanged [@ANCHOR: hams_helpdesk:compute_description_defanged]

`_compute_description_defanged(self)`: the compute method of `description_defanged`; sets it to `defang_html(description)` for a ticket
whose links are defanged, and to the description for every other ticket. No return value, no side effects, cannot fail.

### notify_layout_defanged [@ANCHOR: hams_helpdesk:notify_layout_defanged]

Purpose: the notification email about a message on a phishing ticket has no live link in the ticket's text. Odoo calls
`_notify_by_email_render_layout()` once per group of recipients for every email about a message on a thread; this override is on
`hams_helpdesk.ticket`, so it is the one place every email about a ticket's message passes.

| Argument | Type | Meaning |
|---|---|---|
| `message` | `mail.message` | The message being mailed. |
| `recipients_group` | `dict` | Odoo's recipient group (passed through). |
| `msg_vals` | `dict` or `False` | Odoo's message values; its `body`, when present, is the body to find. |
| `render_values` | `dict` or `None` | Odoo's layout values (passed through). |

Returns the rendered layout (`Markup`). For a ticket whose links are not defanged it is exactly Odoo's. Otherwise the message body inside
the layout is replaced by the warning (`DEFANG_WARNING_TEXT`) followed by `defang_html(body)`, and the layout's own links (the button that
opens the ticket) are kept. Fails safe: if the body is not found verbatim in the rendered layout, the whole layout is defanged instead
(the button is lost, no link survives). Errors: none of its own. Side effects: none; the email is created by Odoo after it returns.
Tested by `test_08` to `test_11`.

### message_defanged_ticket_ids [@ANCHOR: hams_helpdesk:message_defanged_ticket_ids]

`mail.message._defanged_ticket_ids()` (`models/mail_message_defanged.py`): among the messages in `self`, the ids of the tickets whose links
are shown as text (`triage_verdict` is `phishing` and `triage_overridden` is false). One search for the whole recordset, run as the
helpdesk service account (`hams_helpdesk.user_helpdesk_service`) so the answer does not depend on what the reader may see. Returns a `set` of
`int`; empty when no message is on a ticket. Read only.

### message_to_store_defanged [@ANCHOR: hams_helpdesk:message_to_store_defanged]

Purpose: the chatter. The Odoo web client is sent message data by `mail.message._to_store(store, fields, **kwargs)`; this override changes
only the `body` of messages on a defanged ticket.

| Argument | Type | Meaning |
|---|---|---|
| `store` | `Store` | Odoo's response builder (passed through). |
| `fields` | `list` | The field names asked for. Copied, never changed in place. When it contains `"body"` and a message is on a defanged ticket, `"body"` is replaced by a `Store.Attr` that sends `defang_html(body)` for those messages and the stored body for the others. |
| `**kwargs` | | `format_reply`, `msg_vals`, `add_followers`, `followers`: passed through. |

Returns what Odoo's returns (`None`). The stored `mail.message.body` is never changed; reads that do not go through the store (a direct
`read(["body"])`) see the stored text. Tested by `TestPhishingLinkDefangTickets` `test_03` to `test_06`.

### view_phishing_banner [@ANCHOR: hams_helpdesk:view_phishing_banner]

In the ticket form (`views/helpdesk_ticket_views.xml`): a red banner (`o_phishing_banner`, visible when `links_defanged`) with the warning,
and the Description page shows `description_defanged` (read only) instead of `description` while `links_defanged`. Tested by
`test_07_the_warning_text_is_in_the_staff_form_banner`.

## Recent Improvements
- Verified and linked all semantic anchors for traceability.
- Added shift handoff verification to operator tours.
- Enhanced portal UI visibility for tickets.


## Scanning for raw ticket text in AI readers

The static rule (`ai_reader_rule.py`, run by `test_ai_reader_sites`) scans only the files git tracks
(`[@ANCHOR: hams_helpdesk:ai_reader_tracked_files]`, `[@ANCHOR: hams_helpdesk:ai_reader_candidate_files]`), so a stray
untracked directory (a leftover nested checkout) cannot fail it. `scan_tree(root)` is the whole-repository entry point: it
runs the rule over every non-test Python file the scan picks and returns one line per ticket-like text read that skips the
safe view, and the repository passes only when that list is empty [@ANCHOR: hams_helpdesk:ai_reader_sites_rule]. Outside a git checkout it walks the directory instead.
Outgoing mail that fails to send, and failures processing an incoming bounce, are reported the same way
(source "Outgoing / incoming mail"; counts and fixed labels only, never an address or content).
Failures of external daemons (RabbitMQ consumers, sync and ingest programs) reach the same tickets (source
"External daemon") through a systemd `OnFailure=` template unit and the `reporting_failures` helper; see
`daemon/README.md`.
