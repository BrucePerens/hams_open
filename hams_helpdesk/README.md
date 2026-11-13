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
    - `POST /webhook/github` accepts GitHub `dependabot_alert` (created or reopened) and `workflow_run` (completed with a failure) events whose `X-Hub-Signature-256` HMAC (keyed-hash message authentication code) matches the `hams_helpdesk.github_webhook_secret` system parameter, and opens one ticket per alert or run URL. A repeat delivery for a URL whose ticket is still open adds nothing; once that ticket is Resolved or Closed, a new delivery opens a new ticket. With no secret configured, every request is refused with 403.
    - The `pager_duty` module opens tickets for its incidents (see documentation.html).
- **Child-safety mandatory reports**: the ticket type `csam_enticement_trafficking` drives the NCMEC (US National Center for Missing & Exploited Children) CyberTipline reporting workflow (18 U.S.C. Sec. 2258A). Such a ticket is forced to Critical priority, every Helpdesk Manager is notified, and a report packet is assembled on the ticket. Portal users cannot choose this type, and the external AI triage account's record rule does not include it, so that account cannot read it. See documentation.html.
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

* **Ticket Lifecycle (`[@ANCHOR: helpdesk_ticket_lifecycle]`)**: the model `hams_helpdesk.ticket` (`models/helpdesk_ticket.py`, inheriting `mail.thread` and `mail.activity.mixin`) carries every issue. Its `stage` Selection (required, default `new`, tracked) holds New, In Progress, Resolved, Closed and, last, Spam / Phishing (Quarantined), a side lane for flagged inbound mail. Staff may write any stage directly; the code enforces no order. A portal user's write of `stage` is refused with `AccessError`. Every stage change except one into `spam` posts a customer-visible "Ticket Update" message to the ticket's customer, when it has one.

* **Verified by [@ANCHOR: test_01_ticket_creation_and_routing]** (`test_01_ticket_creation_and_routing` in `tests/test_helpdesk_core.py`): patches the bus and, when `pager_duty` is installed, the on-duty lookup; creates a ticket for a portal member; asserts the ticket is assigned to the on-duty manager (left unassigned when `pager_duty` is absent) and that the customer's partner is subscribed to the thread so later stage changes reach them by mail. It also stands as the test for the lifecycle and creation anchors.

* **Hidden-text colour checks (`[@ANCHOR: helpdesk_untrusted_css_number]`)**: a CSS number in attacker-written style or colour text is read as a finite float or as nothing, so the hidden-text detector never raises on `nan`, `inf` or a 400-digit value. `_css_number(text)` takes a str (the number part of a CSS value) and returns a finite `float`, or None for `nan`, `inf`, a number whose size overflows to infinity, or text such as `1.2.3` and `.` that the colour patterns let through. Pure, never raises; `_parse_color` uses it for `rgb()`, `rgba()` alpha and `hsl()` parts, so hostile style text cannot crash the hidden-text check. Tests: `tests/test_untrusted_text_hsl_colours.py` and `tests/test_untrusted_text_named_colours.py`.

* **Ticket Creation (`[@ANCHOR: helpdesk_ticket_creation]`)**: Intercepts the ORM `create` method to automatically execute pre-shift CC logic and route to the currently on-duty personnel based on calendar availability. "Pre-shift CC" subscribes the users returned by `calendar.event.get_upcoming_duty_shifts()` (people whose shift starts soon) and sends them an internal "Shift CC" note. This module's own version of that method returns no shifts and no module in this repository overrides it, so the step currently does nothing.

* **Creation checks (test `test_01_ticket_creation_and_routing`, anchor `[@ANCHOR: test_01_ticket_creation_and_routing]`)**: the same test creates through the plain ORM `create()` with a `partner_id` and no `user_id` and no website, so it exercises these steps of `create()`: the on-duty assignee is looked up once before the batch (a lookup failure is logged and the ticket stays unassigned), the callsign is copied from the partner, the triage verdict fields are dropped from caller values, and `_automated_routing_and_notification()` then subscribes the customer. A ticket type of `server_exception` or a stage of `spam` skips the paging.

* **Shift Handoff Initiation (`[@ANCHOR: helpdesk_shift_handoff]`)**: `hams_helpdesk.ticket.action_shift_handoff()` (a `@public_rpc` button method, no arguments, exactly one ticket or `ensure_one()` raises) returns an `ir.actions.act_window` dict that opens the `hams_helpdesk.shift_handoff` form in a modal (`target: "new"`) with the defaults `default_ticket_id` (this ticket) and `default_old_user_id` (the current assignee, or False). It writes nothing and elevates nothing. The form header button is hidden once the ticket is Resolved or Closed.

* **Verified by [@ANCHOR: test_02_shift_handoff_wizard]** (`tests/test_helpdesk_core.py`): creates a ticket assigned to the manager and a second operator, fills the wizard (`ticket_id`, `old_user_id`, `new_user_id`, `handoff_notes`), calls `action_confirm_handoff()` and asserts that `ticket.user_id` is now the new operator and that the chatter holds the "Official Shift Handoff Executed" text and the notes. Another test in the file asserts markup in the notes is escaped. The operator tour clicks the same button.

* **Handoff Execution (`[@ANCHOR: helpdesk_handoff_execution]`)**: `hams_helpdesk.shift_handoff.action_confirm_handoff()` on the transient wizard (`models/shift_handoff.py`; fields `ticket_id`, `old_user_id` read only, `new_user_id` and `handoff_notes`, both required). Under the `hams_helpdesk.user_helpdesk_service` account it writes `user_id` with `mail_notrack`, then posts a briefing (relinquished by, accepted by, the notes, all escaped through `markupsafe.Markup.format`) as an internal note (`mail.mt_note`, so the customer's portal thread never shows it) addressed to the new assignee. It returns `act_window_close`. A missing service account raises; nothing is caught.

* **Verified by [@ANCHOR: test_02_shift_handoff_wizard]** (second citation, execution side): the same test proves the write happened under the service account (ownership changed although the caller is an ordinary operator) and that the briefing was posted to the ticket's own thread; the escaping test creates a handoff whose notes contain a `<script>` element and asserts the stored body holds `&lt;script&gt;` and never the raw tag.

* **Documentation Injection (`helpdesk_doc_injection`)**: the `knowledge_docs` list in `__manifest__.py` declares each page as `{name, path, icon, category}` plus, for the member manual, `public` and `help_topic`. The member manual (`data/member_manual.html`, public) is the only member-facing entry; `data/documentation.html` and the story and journey files under `docs/` are internal. The `zero-sudo` documentation facility reads the list at install and upgrade and loads the pages into the knowledge base; this module has no loader code of its own. The facility's exact behaviour is documented with that module.

* **Verified by [@ANCHOR: test_05_doc_injection]**: no test method in this module carries this tag today; the nearest real check is `test_01_manual_is_a_published_member_article` in `tests/test_member_manual.py`, which asserts exactly one article titled "Getting help: Support tickets" exists, with audience `member`, published and with a help topic, and that the developer manual's articles are internal. To-do: add a test that every `knowledge_docs` path exists and is loaded, and cite this tag in it.

* **Multi-Website & Multi-Company Awareness (`[@ANCHOR: helpdesk_multi_website]`)**: `website_id` (Many2one `website`, `ondelete="restrict"`) records where a ticket was created and `company_id` (required, default the current company) says whose it is. In `create()` a missing `website_id` is taken from the context key `website_id`, and a missing `company_id` from that website's company. The portal list, count and detail routes show only tickets whose website is the current one or empty; a detail request for another site's ticket redirects to `/my`. A portal write of either field is refused.

* **Verified by [@ANCHOR: test_06_multi_website_awareness_logic]** (`tests/test_helpdesk_multi_website.py`): creates three tickets for one portal partner (website 1, website 2, none), then searches as the portal user with the context `website_id` set to each site in turn, and asserts the user sees their site's ticket and the website-less one but never the other site's. The same anchor is also cited on `portal_my_tickets` in `controllers/portal.py`.

* **Micro-Privilege Security (`[@ANCHOR: helpdesk_micro_privilege]`)**: Access is strictly controlled via record rules and explicit field-level security in the ORM: `write()` refuses a portal user's change to stage, assignee, priority, customer, ticket type, website, company, calendar event, the archive flag, or any `ncmec_*` field.

* **Portal write checks (`[@ANCHOR: test_05_portal_write_restrictions]`)**: `test_05_portal_write_restrictions` in `tests/test_helpdesk_core.py` creates a ticket and, as a portal user, tries to write `stage` and `user_id`; both writes must raise `AccessError`. It then writes the description, which is allowed, and checks it changed. `test_05b_portal_cannot_reassign_partner_id` adds the rule that `partner_id` is restricted too, because a portal user could otherwise move the ticket into a stranger's portal.

* **Helpdesk Operator Workflow (`[@ANCHOR: helpdesk_operator_tour]`)**: the tour `static/src/tours/helpdesk_operator_tour.js`, run by `test_helpdesk_operator_tour` in `tests/test_ui_tours.py` as the manager user `hd_manager_tour` from the ticket action: it opens New, fills the subject, saves, clicks the Shift Handoff button, chooses the next assignee, types notes, confirms, waits for the modal to close and for the form to reload. It stands as the test of the backend form, list and handoff views.

* **Helpdesk Portal Facility (`[@ANCHOR: helpdesk_portal_tour]`)**: the tour `static/src/tours/helpdesk_portal_tour.js`, run by `test_helpdesk_portal_tour` as the portal user `portal_cust_tour` from `/my/tickets`: it clicks New ticket, fills subject, callsign and description, submits, checks the detail page (breadcrumb, callsign, status badge), closes the ticket and checks the badge changed. It stands as the test of the portal list, new, detail and close pages.

* **Portal Ticket Closure (`[@ANCHOR: helpdesk_portal_close]`)**: `hams_helpdesk.ticket.action_portal_close()` (`@api.private`, one ticket) closes the customer's own ticket. It raises `AccessError` unless the caller's partner is the ticket's customer or the caller is in `group_helpdesk_user`. A ticket not yet closed is written to `closed` under the helpdesk service account with `mail_notrack`, and a "Ticket closed by customer." note is posted; an already closed ticket does nothing. The route is `POST /my/ticket/<id>/close` (CSRF protected, login required), which redirects to the ticket afterwards.

* **Verified by [@ANCHOR: test_portal_close_ticket]** (`tests/test_ui_tours.py`): creates a ticket for the portal user, logs in as that user, reads the CSRF token from `/my/tickets`, posts to `/my/ticket/<id>/close` and asserts the stage is `closed`. A tour step in `helpdesk_portal_tour.js` does the same through the browser.


## Stories and Journeys

### Ticket Lifecycle Management ([@ANCHOR: helpdesk_ticket_lifecycle])
**Goal**: Efficiently track and resolve system issues while maintaining a clear audit trail.
1.  **Incoming Request**: A new ticket is created, either manually or via automated incident detection.
2.  **Automated Routing**: The system identifies the currently on-duty administrator (`[@ANCHOR: helpdesk_ticket_creation]`) and assigns the ticket.

3.  **Progression**: The operator manages tickets via the list ([@ANCHOR: helpdesk_ticket_list]) and form ([@ANCHOR: helpdesk_ticket_form]) views, moving through stages: New -> In Progress -> Resolved -> Closed. A fifth stage, Spam / Phishing (Quarantined), is a side lane for flagged inbound mail, not a normal step (see documentation.html).
4.  **Customer Communication**: Every stage change except a move into Spam / Phishing triggers an automated update to the reporter (the ticket's Customer), if the ticket has one.

**Interface**: this story spans `create()` (assignment, see Ticket Creation above), the stage field and `write()` (see Ticket Lifecycle above), and two backend views in `views/helpdesk_ticket_views.xml`. The list view `view_hams_helpdesk_ticket_list` (anchor `[@ANCHOR: helpdesk_ticket_list]`, in the XML as `COMM_helpdesk_ticket_list`) shows tickets for staff and is checked by `test_helpdesk_operator_rendering`, which loads it with `get_view`. The form view `view_hams_helpdesk_ticket_form` (anchor `[@ANCHOR: helpdesk_ticket_form]`) holds the stage statusbar, the Shift Handoff and Close Ticket buttons, and the phishing banner; the same test loads it too.

**Errors and limits**: the story has no function of its own. A missing `hams_helpdesk.user_helpdesk_service` account makes ticket creation raise, by design; a failed on-duty lookup is only logged. Which tickets a user may see at all is decided by the record rules, not by this story. Where the views and tour are checked, see `test_ui_tours.py`.

### Incident Resolution Journey ([@ANCHOR: journey_incident_resolution])
**Goal**: Complete the lifecycle of a critical incident from detection to resolution.
1.  **Detection**: System monitor detects a service failure.
2.  **Creation**: A Helpdesk ticket is created.
3.  **Assignment**: The system auto-assigns the ticket to the SRE currently "On-Duty".
4.  **Investigation**: SRE updates stage to "In Progress".
5.  **Resolution**: SRE fixes the issue, updates stage to "Resolved".
6.  **Closure**: After verification, the ticket is moved to "Closed".

**Interface**: this journey is the lifecycle above seen end to end; it has no code of its own. Its steps are `create()` with automatic on-duty assignment, then staff `write()` of `stage` (each move except into Spam mails the customer), then `action_close()` (a staff-only header button on the backend form that writes `stage` to `closed` under the caller's own rights, so the stage-change mail-back fires as for a statusbar click) or a portal close. Nothing in the journey is automatic after assignment: a human moves each stage. Tested by `test_01_ticket_creation_and_routing` and `test_action_close_sets_stage_and_is_usable_by_non_manager_staff`.


### Shift Handoff Protocol ([@ANCHOR: journey_shift_handoff])
**Goal**: Ensure seamless continuity of operations when operators rotate shifts.
1.  **Initiation**: The outgoing operator selects "Shift Handoff" on an active ticket (`[@ANCHOR: helpdesk_shift_handoff]`).
2.  **Context Capture**: A wizard appears requiring the selection of the next assignee and detailed handoff notes. The Shift Handoff button is hidden once a ticket is Resolved or Closed.
3.  **Execution**: Upon confirmation, the system atomically updates ownership and logs a briefing (`[@ANCHOR: helpdesk_handoff_execution]`).

4.  **Verification**: The handoff is recorded in the chatter and the operator workflow is validated by [@ANCHOR: helpdesk_operator_tour].

**Interface**: the three calls are `action_shift_handoff()` on the ticket, the wizard form (`views/shift_handoff_views.xml`, view `view_hams_helpdesk_shift_handoff_form`, with a Confirm Handoff button bound to `action_confirm_handoff`) and `action_confirm_handoff()` on the wizard, all described under the Shift Handoff bullets above. Errors: an empty or multi-record call raises `ValueError` through `ensure_one()`; both `new_user_id` and `handoff_notes` are required, so the form will not submit without them. Side effects: one write of `user_id` and one internal chatter note that notifies only the new assignee. Verified by `test_02_shift_handoff_wizard` and the operator tour.


### Portal Self-Service ([@ANCHOR: journey_portal_self_service])
**Goal**: Empower customers to manage their own tickets.
1.  **Submission**: Customer creates a ticket via the portal ([@ANCHOR: helpdesk_portal_new]).

2.  **Review**: Customer views their tickets in the list ([@ANCHOR: helpdesk_portal_list]) and detail ([@ANCHOR: helpdesk_portal_detail]) views.

3.  **Closure**: Customer closes the ticket when resolved ([@ANCHOR: helpdesk_portal_close]).

4.  **Verification**: Validated by [@ANCHOR: helpdesk_portal_tour].

**Interface**: the portal routes are in `controllers/portal.py`, all `auth="user"`: `GET /my/tickets` and `/my/tickets/page/<n>` (list, 20 per page, newest first, anchor `helpdesk_portal_list` in the template as `COMM_helpdesk_portal_list`), `GET /my/tickets/new` (the form, `helpdesk_portal_new`; an optional `?ticket_type=` pre-selects a category, checked against the portal-visible types), `POST /my/tickets/submit` (CSRF (cross-site request forgery) protected; creates the ticket under the helpdesk service account), `GET /my/ticket/<id>` (detail, `helpdesk_portal_detail`; another member's ticket or another website's redirects to `/my`) and `POST /my/ticket/<id>/close`. The child-safety and automatic-exception ticket types are never offered or accepted from the portal. Tested by `test_helpdesk_portal_rendering`, `test_portal_close_ticket` and the portal tour.

**Limits and security**: every route needs a logged-in user, and every read runs under the `hams_helpdesk.user_helpdesk_service` account and the website's company, filtered by hand to the member's own partner and website, so the member's own rights never decide what is listed. A mismatch of owner is not an error page but a redirect to `/my`, which also hides whether the ticket exists. The submit route re-checks the posted ticket type against the portal-visible types (a tampered hidden field falls back to `general`), requires a subject (an empty one redirects back to the form) and fills the callsign from the member's profile when the field is blank.


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

### defang_text_node [@ANCHOR: hams_helpdesk:defang_text_node]

Purpose: defangs the URLs written out in a piece of plain text; used by `defang_html()` on every text node and on the text after
every element, and for the whole text when a fragment cannot be parsed.

| Argument | Type | Meaning |
|---|---|---|
| `text` | `str` or falsy | Plain text. Borrowed, never modified. |

Returns a `str`: each `http://`, `https://`, `ftp://`, `mailto:` or `www.` address (up to the next white space, `<`, `>` or quote) is replaced by
`defang_url_text()` of it; everything else, including numbers such as `1.2`, is unchanged. A falsy `text` is returned as it is (`""`
or `None`). A full stop right after an address is taken as part of it and is defanged too (harmless). Raises nothing, no side effects. Tested by `test_01b_urls_in_plain_text_are_defanged_and_the_rest_is_left_alone`.

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

### compute_links_defanged [@ANCHOR: hams_helpdesk:compute_links_defanged] Declared `@api.depends("triage_verdict", "triage_overridden")`, so Odoo recomputes it only when one of them changes. `links_defanged` is a non-stored `Boolean`; because it is computed on read, a changed verdict or a person's override shows at once. It runs as the reader (no elevation), and it feeds the staff banner (`view_phishing_banner`) and the notification email layout (`_notify_by_email_render_layout`).

`_compute_links_defanged(self)`: the compute method of `links_defanged`; sets it to `_triage_links_defanged()` for each ticket in `self`.
No other arguments or return value, no side effects, cannot fail.

### compute_description_defanged [@ANCHOR: hams_helpdesk:compute_description_defanged] Declared `@api.depends("description", "triage_verdict", "triage_overridden")`. `description_defanged` is a non-stored read-only `Html` field (`sanitize=False`: the text is already the filter's output and `defang_html` returns safe HTML). It is never written back, so the stored description keeps its real links; staff see the defanged copy only in the form. Runs as the reader.

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

### view_phishing_banner [@ANCHOR: hams_helpdesk:view_phishing_banner] The banner element is an `alert-danger` div with the invisible helper field `links_defanged` beside it. Its text tells staff that the verdict is phishing, that `hxxp`, `hxxps` and `[.]` stand for `http`, `https` and a dot, and that moving the ticket out of the Spam stage makes the links work again. It adds no field and no action; the data comes from `_compute_links_defanged`, and only users who can open the backend form see it.

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

## The untrusted-text filter (`models/untrusted_text.py`)

Pure Python (no Odoo import); the policy and the list of vectors are in `docs/TICKET_PROMPT_INJECTION.md`, the hostile corpus in `tests/test_untrusted_text.py` and the direct tests below in `tests/test_untrusted_text_api.py`.

- `[@ANCHOR: hams_helpdesk:untrusted_result_init]` `Result(text="", plain=None, findings=None, removed=0, score=0)` holds what the filter found: the clean `text` (or sanitized HTML), its visible `plain` text (defaults to `text`), the `findings` list (each result gets its own list), the number of characters `removed` and the suspicion `score`. **Tests:** `TestUntrustedTextBuildingBlocks.test_01_result_holds_text_findings_and_a_suspicion_flag`.

- `[@ANCHOR: hams_helpdesk:untrusted_result_iter]` `iter(Result)` yields `(text, removed)`, the tuple `clean_untrusted_text` always returned, so older callers can still unpack a result. Takes no arguments and returns an iterator over the two-tuple, so `text, removed = result` works. The findings and the score are not part of the unpacking: a caller that needs them reads the attributes. Pure, never raises. **Tests:** `TestUntrustedTextBuildingBlocks.test_01_result_holds_text_findings_and_a_suspicion_flag`.

- `[@ANCHOR: hams_helpdesk:untrusted_result_suspicious]` `Result.suspicious` is True when `score` is at or over `SUSPICION_THRESHOLD` (5); a suspicious item is never auto-triaged, auto-answered or acted on by an AI. A read-only property with no arguments that returns `bool` and never changes `score`. The ticket mixin stores the result in the ticket's `suspicious` field, which the AI-reader rule and the triage daemon read: a suspicious ticket comes back from `mcp_safe_read` with no text at all. **Tests:** `TestUntrustedTextBuildingBlocks.test_01_result_holds_text_findings_and_a_suspicion_flag`.

- `[@ANCHOR: hams_helpdesk:untrusted_sanitizer_init]` `_Sanitizer(location)` is the allow-list HTML parser behind `sanitize_html`; it starts with white as the background colour and empty output, plain-text, dropped-text and finding lists. `location` (str) is the label copied into every finding, normally the field name. The instance state is `out` and `text` (sanitized and plain fragments), `dropped` (reason and text pairs), `findings`, the open-element `stack`, `drop_depth` and `drop_reason`, the background `bg`, the counters `dropped_text`, `kept_text`, `comments` and `link_mismatch`, `attr_text`, `link_stack` and `in_pre`. One instance parses one document and is not shared between threads; only `sanitize_html` creates it. **Tests:** `TestUntrustedTextHtmlSanitizer.test_01_sanitizer_methods_keep_allowed_tags_and_drop_everything_else`.

- `[@ANCHOR: hams_helpdesk:untrusted_sanitizer_attr_texts]` `_Sanitizer._attr_texts(tag, attrs)` records text an attacker could hide in attributes: CSS `content:`, alt, title, aria and `data-` values of four or more characters, `data:` sources, event handlers and `javascript:`/`vbscript:`/`data:` links. **Tests:** `TestUntrustedTextHtmlSanitizer.test_02_comments_declarations_instructions_and_attributes_are_findings`.

- `[@ANCHOR: hams_helpdesk:untrusted_sanitizer_note_drop]` `_Sanitizer._note_drop(reason, text)` records stripped, non-empty text under its reason and counts its length; blank text is ignored. `reason` is a short fixed word (`html_comment`, `element_script`, `img_alt`), `text` the removed text. The text is stripped first; it then appends `(reason, text)` to `dropped` and adds its length to `dropped_text`, which `sanitize_html` turns into findings with a short excerpt. Returns None and cannot raise. **Tests:** `TestUntrustedTextHtmlSanitizer.test_02_comments_declarations_instructions_and_attributes_are_findings`.

- `[@ANCHOR: hams_helpdesk:untrusted_sanitizer_handle_comment]` `_Sanitizer.handle_comment(data)` removes an HTML comment and records its text as hidden. Called by `HTMLParser` with `data` (str), the text between `<!--` and `-->`. It increments the `comments` counter and passes the reason `html_comment` and the text to `_note_drop`; nothing is emitted to the output. Returns None. **Tests:** `TestUntrustedTextHtmlSanitizer.test_02_comments_declarations_instructions_and_attributes_are_findings`.

- `[@ANCHOR: hams_helpdesk:untrusted_sanitizer_handle_data]` `_Sanitizer.handle_data(data)` keeps and escapes visible text, and counts text inside a dropped element as hidden under its reason. `data` (str) is a run of text with character references already converted (`convert_charrefs=True`). Inside a dropped subtree (`drop_depth` above zero) it goes to `_note_drop` with that subtree's reason and is not kept. Otherwise its stripped length is added to `kept_text`, an HTML-escaped copy (quotes not escaped) to `out`, the raw text to `text`, and the text to the open link's list for the host check. Returns None. **Tests:** `TestUntrustedTextHtmlSanitizer.test_01_sanitizer_methods_keep_allowed_tags_and_drop_everything_else`.

- `[@ANCHOR: hams_helpdesk:untrusted_sanitizer_handle_endtag]` `_Sanitizer.handle_endtag(tag)` closes the nearest open element of that name (an unmatched end tag is ignored), restores the background, and for a link flags text that names a different host than the `href` (`link_text_href_mismatch`, weight 3). **Tests:** `TestUntrustedTextHtmlSanitizer.test_01_sanitizer_methods_keep_allowed_tags_and_drop_everything_else`.

- `[@ANCHOR: hams_helpdesk:untrusted_sanitizer_handle_pi]` `_Sanitizer.handle_pi(data)` removes a processing instruction and records its text as hidden. Called by `HTMLParser` for `<?...>` with `data` (str). It passes the reason `processing_instruction` and the text to `_note_drop`; nothing is emitted and no counter changes. Returns None. **Tests:** `TestUntrustedTextHtmlSanitizer.test_02_comments_declarations_instructions_and_attributes_are_findings`.

- `[@ANCHOR: hams_helpdesk:untrusted_sanitizer_handle_startendtag]` `_Sanitizer.handle_startendtag(tag, attrs)` treats `<tag/>` as a start tag followed at once by its end tag (void tags have no end). `tag` and `attrs` are as for `handle_starttag`. It calls `handle_starttag`, then `handle_endtag` for the same tag unless the tag is a void tag (`br`, `hr`, `img` and the like), which never has an end tag. Returns None. Side effects are those of the two calls. **Tests:** `TestUntrustedTextHtmlSanitizer.test_01_sanitizer_methods_keep_allowed_tags_and_drop_everything_else`.

- `[@ANCHOR: hams_helpdesk:untrusted_sanitizer_handle_starttag]` `_Sanitizer.handle_starttag(tag, attrs)` drops an element and its subtree when its tag is never shown or `_hidden_reason` finds it hidden, tracks the background colour, and emits only allow-listed tags with no attributes except a checked `a href` carrying `rel="nofollow noopener noreferrer"`. **Tests:** `TestUntrustedTextHtmlSanitizer.test_01_sanitizer_methods_keep_allowed_tags_and_drop_everything_else`.

- `[@ANCHOR: hams_helpdesk:untrusted_sanitizer_unknown_decl]` `_Sanitizer.unknown_decl(data)` removes a CDATA block or other declaration and records its text as hidden. Called by `HTMLParser` for a `<![CDATA[...]]>` style declaration with `data` (str). It increments `comments` and records the text under the reason `cdata_or_decl`; a plain `<!DOCTYPE>` never reaches it (`HTMLParser.handle_decl` does nothing). Returns None. **Tests:** `TestUntrustedTextHtmlSanitizer.test_02_comments_declarations_instructions_and_attributes_are_findings`.

- `[@ANCHOR: hams_helpdesk:untrusted_add]` `_add(findings, vector, location, count, excerpt="", weight=0)` appends one finding dict (vector, location, count, excerpt, weight); a zero count with no weight is not a finding, and nothing is added once `MAX_FINDINGS` (40) are held. **Tests:** `TestUntrustedTextBuildingBlocks.test_02_findings_are_excerpted_capped_and_scored`.

- `[@ANCHOR: hams_helpdesk:untrusted_bucket]` `_bucket(ch)` names the finding vector for a character that must be removed (line separator, Unicode tag, variation selector, bidi control, invisible filler, zero width, control, format, private use, unassigned or surrogate) and returns None for a character to keep (newline and tab included). **Tests:** `TestUntrustedTextBuildingBlocks.test_03_characters_are_bucketed_and_scripts_told_apart`.

- `[@ANCHOR: hams_helpdesk:untrusted_contrast]` `_contrast(a, b)` returns the WCAG contrast ratio (1.0 to 21.0) between two RGB tuples; below 1.6 the text counts as unreadable. `a` and `b` are RGB tuples of three 0 to 255 integers, in either order. The result is `(lighter + 0.05) / (darker + 0.05)` from `_luminance`, a float from 1.0 (same colour) to 21.0 (black on white). `_hidden_reason` treats text under 1.6 against the background as unreadable. Pure; a malformed tuple raises `ValueError` or `TypeError`, which callers avoid by passing only `_parse_color` output. **Tests:** `TestUntrustedTextBuildingBlocks.test_07_css_colour_and_size_helpers`.

- `[@ANCHOR: hams_helpdesk:untrusted_decoded_variants]` `_decoded_variants(text)` returns `(kind, decoded text)` pairs a reader could be asked to decode: base64, hex, percent, HTML entities, quoted-printable, and always rot13, reversed and leetspeak; for scanning only. `text` is a str. Returns a list of `(kind, decoded)` pairs: `base64` (up to 20 candidate blobs of 24 or more characters, standard or URL-safe alphabet, kept when the result is UTF-8 and over 90 percent printable), `hex` (up to 10), `percent`, `html_entity` and `quoted_printable` when their markers appear, plus rot13, reversed and leetspeak variants always. The pairs are only scanned by `scan_encodings`; they are never stored or shown. Pure, never raises on bad encodings. **Tests:** `TestUntrustedTextBuildingBlocks.test_05_phrase_and_encoding_scans_look_at_decoded_text_without_returning_it`.

- `[@ANCHOR: hams_helpdesk:untrusted_excerpt]` `_excerpt(value)` returns a short printable excerpt, safe for a staff banner or a log: characters that are not printable or are one of `<>&"'` become `?`, whitespace runs collapse, and it is cut to `EXCERPT_CHARS` (60) with `...`. **Tests:** `TestUntrustedTextBuildingBlocks.test_02_findings_are_excerpted_capped_and_scored`.

- `[@ANCHOR: hams_helpdesk:untrusted_hidden_reason]` `_hidden_reason(tag, attrs, bg)` returns why an element's text would not be visible (hidden attribute, aria-hidden, closed details, hidden input, display none, visibility, opacity, tiny font, zero box with overflow, clipping, off-screen offset or indent, scale 0, zero line height, transparent or low-contrast colour against the background `bg`), or None when it would be shown. **Tests:** `TestUntrustedTextBuildingBlocks.test_08_hidden_reasons_background_and_links`.

- `[@ANCHOR: hams_helpdesk:untrusted_is_phrase]` `_is_phrase(vector)` is True for a heuristic phrase vector (one of the `_PHRASES` names, or any `split_` name) and False for hidden-content vectors; it decides how `_score` weighs a finding. `vector` is the finding's vector name (str). The set of phrase names is built once from `_PHRASES` on first use and kept in a module global, so the first call is the only one that does work. Returns `bool`. Pure apart from that cache, and it cannot raise for a string. **Tests:** `TestUntrustedTextBuildingBlocks.test_02_findings_are_excerpted_capped_and_scored`.

- `[@ANCHOR: hams_helpdesk:untrusted_luminance]` `_luminance(rgb)` returns the WCAG relative luminance (0.0 black to 1.0 white) of an RGB tuple. `rgb` is a tuple of three 0 to 255 numbers (red, green, blue). Each channel is converted from sRGB to linear light and weighted 0.2126, 0.7152 and 0.0722, as WCAG defines, giving a float between 0.0 and 1.0. Pure; a tuple of the wrong length raises `ValueError`. **Tests:** `TestUntrustedTextBuildingBlocks.test_07_css_colour_and_size_helpers`.

- `[@ANCHOR: hams_helpdesk:untrusted_mixed_script_words]` `_mixed_script_words(text)` returns the words that mix Latin letters with letters of another listed script, the homoglyph trick (`pаypal` with a Cyrillic a); a word wholly in one script is not returned. `text` is a str. Splits it into `\w+` words and, per word, collects the script of every letter through `_script`; a word is returned when LATIN is among its scripts and at least one other listed script is too. Returns the list of such words in order, with repeats. Digits and punctuation are ignored. Pure, never raises. **Tests:** `TestUntrustedTextBuildingBlocks.test_03_characters_are_bucketed_and_scripts_told_apart`.

- `[@ANCHOR: hams_helpdesk:untrusted_new_bg]` `_new_bg(attrs, bg)` returns the background colour an element sets (style `background-color`, `background` or the `bgcolor` attribute) as an RGB tuple, or `bg` unchanged when it sets none or one that is not a plain colour. **Tests:** `TestUntrustedTextBuildingBlocks.test_08_hidden_reasons_background_and_links`.

- `[@ANCHOR: hams_helpdesk:untrusted_parse_color]` `_parse_color(value)` reads a CSS colour (named, `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb()`/`rgba()`, `hsl()` near white) as an RGB tuple, `"invisible"` for a nearly transparent one, or None when it is not a colour a reader can compare; a number that is not finite is None, never an exception. **Tests:** `TestUntrustedTextBuildingBlocks.test_07_css_colour_and_size_helpers`.

- `[@ANCHOR: hams_helpdesk:untrusted_part_text]` `_part_text(part)` decodes a mail part to text with its declared charset, replacing bad bytes, and falls back to UTF-8 for an unknown charset; a part with no payload is an empty string. `part` is an `email.message.Message` leaf. `get_payload(decode=True)` returns the transfer-decoded bytes, which are then decoded with the declared charset (default UTF-8), with `replace` for bad bytes; an unknown charset (`LookupError`) or a charset error falls back to UTF-8 with `replace`. Returns `str`, never raises for a leaf, and reads nothing else. The text is unfiltered: callers pass it on to the scanners. **Tests:** `TestUntrustedTextMailAndAiHelpers.test_02_mime_alternatives_and_whole_mail_inspection`.

- `[@ANCHOR: hams_helpdesk:untrusted_safe_href]` `_safe_href(value)` returns the stripped link when its scheme is http or https with a host, or mailto, and None for every other scheme, a relative or host-less address and a value that does not parse. **Tests:** `TestUntrustedTextBuildingBlocks.test_08_hidden_reasons_background_and_links`.

- `[@ANCHOR: hams_helpdesk:untrusted_scan_phrases]` `_scan_phrases(text, location, findings, prefix="")` adds one finding per heuristic phrase that matches (name prefixed with `prefix`) and returns how many phrases matched. `text` (str), `location` (str, the label for findings), `findings` (list, appended to in place) and `prefix` (str, default empty; `split_` for a phrase divided across fields). Each phrase in `_PHRASES` is searched in each view from `_scan_views`; the first view that matches adds one finding (name, location, number of matches, first match as excerpt, the phrase's weight) through `_add` and moves to the next phrase. Returns the number of phrases that matched. Never raises. **Tests:** `TestUntrustedTextBuildingBlocks.test_05_phrase_and_encoding_scans_look_at_decoded_text_without_returning_it`.

- `[@ANCHOR: hams_helpdesk:untrusted_scan_views]` `_scan_views(text)` returns the strings a phrase scan looks at: the folded text, and the NFKC-only lower-cased text as well when it differs (for non-Latin phrases). `text` (str). Returns a tuple of one or two strings: the `fold_for_scanning` form (NFKC, look-alikes mapped to Latin, case-folded) and, only when it differs from the plain NFKC case-folded text, that second form too, so a phrase written in a non-Latin alphabet is still found. Pure, cannot raise. **Tests:** `TestUntrustedTextBuildingBlocks.test_05_phrase_and_encoding_scans_look_at_decoded_text_without_returning_it`.

- `[@ANCHOR: hams_helpdesk:untrusted_score]` `_score(findings)` adds hidden-content weights in full; phrase findings are weak evidence, so on their own they total at most 4 unless one has weight 5 or more or two different phrase vectors are present. **Tests:** `TestUntrustedTextBuildingBlocks.test_02_findings_are_excerpted_capped_and_scored`.

- `[@ANCHOR: hams_helpdesk:untrusted_script]` `_script(ch)` returns the script family name (LATIN, CYRILLIC, GREEK, ARMENIAN, HEBREW, ARABIC, CHEROKEE, COPTIC) of a letter, or None for anything else or an unnamed character. `ch` is one character. The script is taken from the start of its Unicode name (`unicodedata.name`), tested against the module's `_SCRIPT_PREFIXES`. Returns the matching prefix string, or None when the name does not start with one, when the character has no name (the `ValueError` is caught), or when it is not a letter of a listed script. Pure. **Tests:** `TestUntrustedTextBuildingBlocks.test_03_characters_are_bucketed_and_scripts_told_apart`.

- `[@ANCHOR: hams_helpdesk:untrusted_style_dict]` `_style_dict(style)` splits an inline style into a lower-cased property dict, keeping semicolons inside parentheses (`url(a;b)`) in the value; None gives an empty dict. `style` is the text of an inline `style` attribute or None. It is split on semicolons that are not inside parentheses, so `url(a;b)` stays whole; each part with a colon becomes a lower-cased property name and a stripped lower-cased value, later duplicates replacing earlier ones. Returns a `dict` (empty for None or no declarations). Pure, never raises. Used by `_hidden_reason` and `_new_bg`. **Tests:** `TestUntrustedTextBuildingBlocks.test_07_css_colour_and_size_helpers`.

- `[@ANCHOR: hams_helpdesk:untrusted_leading_number]` `_leading_number(value)` returns the number a CSS value starts with as a float (`"-3.5em"` gives -3.5), or None when it starts with no number, with digits and dots that are no number (`"1.2.3"`, `"."`), or is None; a value of hundreds of digits gives `inf`, so a huge offset reads as off-screen and not as unreadable. It is what `_tiny_size` and `_hidden_reason` (offsets and text indent) use. **Tests:** `TestUntrustedTextBuildingBlocks.test_07_css_colour_and_size_helpers`.

- `[@ANCHOR: hams_helpdesk:untrusted_tiny_size]` `_tiny_size(value)` is True when a CSS font size is under 5 px after unit conversion (px, pt, em, rem, %, ex, ch, vw, vh, mm, cm, in) or is `xx-small` or `x-small`. `value` is a lower-cased CSS font-size (str) or None. A leading number with an optional unit is converted to pixels (`pt` times 1.33, `em` and `rem` times 16, `%` times 0.16, `ex` and `ch` times 8, `vw` and `vh` times 5, `mm` times 3.8, `cm` times 38, `in` times 96, a bare number is px) and compared with 5. A value that starts with no number is True only for `xx-small` and `x-small`. A number that is not finite is False. Pure, returns `bool`. **Tests:** `TestUntrustedTextBuildingBlocks.test_07_css_colour_and_size_helpers`.

- `[@ANCHOR: hams_helpdesk:untrusted_attachment_notice]` `attachment_notice(count)` returns the fixed sentence every AI read states: attachments are never read, N were not opened. `count` is the number of attachments (int, or None or any falsy value for zero). It fills the module constant `ATTACHMENT_NOTICE` and returns the str. It never opens or reads an attachment; the sentence exists so a model reading a ticket is told that attachments exist and were withheld. Raises `ValueError` if `count` is a non-numeric string. **Tests:** `TestUntrustedTextMailAndAiHelpers.test_01_html_detection_headers_filenames_and_findings_json`.

- `[@ANCHOR: hams_helpdesk:untrusted_clean_untrusted_text]` `clean_untrusted_text(value, limit)` returns `(clean text, characters removed)`, the original simulated-band signature, through `scan_text`. `value` is any object (a non-string is treated as empty by `scan_text`) and `limit` the character cap, required and positional. Returns the two-tuple `(clean text, characters removed)` from `scan_text`, dropping findings and score. Kept for older callers that unpacked the pair; new code calls `scan_text` and reads the full `Result`. Pure, never raises. **Tests:** `TestUntrustedTextBuildingBlocks.test_06_scan_text_clean_text_and_merge`.

- `[@ANCHOR: hams_helpdesk:untrusted_compare_alternatives]` `compare_alternatives(plain_text, html_text, location="alternative_parts")` returns a `mime_alternative_mismatch` finding (weight 4) when the text/plain and text/html parts of one mail differ by at least eight words and over 40 percent of the longer one, else an empty list. **Tests:** `TestUntrustedTextMailAndAiHelpers.test_02_mime_alternatives_and_whole_mail_inspection`.

- `[@ANCHOR: hams_helpdesk:untrusted_filter_ai_output]` `filter_ai_output(text, allowed_hosts=(), location="ai_output")` makes a model's output safe to store or render: invisible characters, markdown images, HTML tags and reference definitions removed, links and bare URLs kept only for an https address on an allowed host (others become their text or `[link removed]`), with an `ai_output_link_or_image_removed` finding when anything was removed. **Tests:** `TestUntrustedTextMailAndAiHelpers.test_03_prompt_block_and_ai_output_filter`.

- `[@ANCHOR: hams_helpdesk:untrusted_findings_from_json]` `findings_from_json(value)` returns the list of finding dicts in stored JSON, and `[]` for anything that is not JSON or not a list. `value` is the stored JSON text (str, or None for an empty column). It is parsed with `json.loads`; a `TypeError` or `ValueError` gives `[]`, a document that is not a list gives `[]`, and elements that are not dicts are dropped. Returns a list of finding dicts. Pure; it is the reader of what `findings_to_json` wrote into the suspicion record's `findings_json` column. **Tests:** `TestUntrustedTextMailAndAiHelpers.test_01_html_detection_headers_filenames_and_findings_json`.

- `[@ANCHOR: hams_helpdesk:untrusted_findings_to_json]` `findings_to_json(findings)` serialises at most `MAX_FINDINGS` findings as compact ASCII JSON. `findings` is a list of finding dicts. Only the first `MAX_FINDINGS` (40) are written, as compact JSON (no spaces, ASCII only with `\u` escapes), so the stored text never carries raw non-ASCII from an attacker's excerpt. Returns `str`. Raises `TypeError` if a finding holds a value JSON cannot encode, which the filter's own findings never do. **Tests:** `TestUntrustedTextMailAndAiHelpers.test_01_html_detection_headers_filenames_and_findings_json`.

- `[@ANCHOR: hams_helpdesk:untrusted_fold_for_scanning]` `fold_for_scanning(text)` returns the NFKC form with look-alike Cyrillic and Greek letters mapped to Latin and case-folded; it is used only to look for instructions and never reaches a reader. `text` (str). Applies Unicode NFKC, maps each look-alike Cyrillic or Greek letter through the module's `_CONFUSABLES` table to its Latin twin, then case-folds. Returns `str`. The result is only used to look for instructions (`_scan_views`) and is never stored, shown or given to a model. Pure; a non-string raises `TypeError`. **Tests:** `TestUntrustedTextBuildingBlocks.test_03_characters_are_bucketed_and_scripts_told_apart`.

- `[@ANCHOR: hams_helpdesk:untrusted_inspect_mail]` `inspect_mail(message, location="mail")` checks one parsed mail before anything reads it: header allow-list and free-text headers scanned, a repeated singleton header flagged, every text/plain and text/html pair compared, attachment names filtered, attachments counted and never opened, archives and nested messages flagged. It returns a `Result` with empty text; an error during inspection is itself a suspicious finding, so a mail is never dropped or passed as clean. **Tests:** `TestUntrustedTextMailAndAiHelpers.test_02_mime_alternatives_and_whole_mail_inspection`.

- `[@ANCHOR: hams_helpdesk:untrusted_looks_like_html]` `looks_like_html(value)` is True for a string containing a tag or a comment, and False for anything else, including a non-string. `value` is any object. True when it is a `str` that contains something shaped like a tag (`<`, an optional slash, a letter or `!`, then `>`) or the start of a comment, `<!--`. A non-string, or text with only a bare `<` or `>` such as `a < b`, is False. Pure, never raises. `sanitize_any` uses it to pick the HTML sanitizer or the plain-text scan. **Tests:** `TestUntrustedTextMailAndAiHelpers.test_01_html_detection_headers_filenames_and_findings_json`.

- `[@ANCHOR: hams_helpdesk:untrusted_merge]` `merge(results)` combines per-field results and returns `(findings capped at MAX_FINDINGS, characters removed summed, score summed and capped at 100)`. `results` is an iterable of `Result` (read twice, so pass a list). Returns `(findings, removed, score)`: every result's findings in order, cut to `MAX_FINDINGS`; the sum of `removed`; the sum of `score` capped at 100. It does not build a `Result`, so the plain text of the parts is not carried. Pure. The ticket mixin uses it to give one record-level verdict over subject, callsign and description. **Tests:** `TestUntrustedTextBuildingBlocks.test_06_scan_text_clean_text_and_merge`.

- `[@ANCHOR: hams_helpdesk:untrusted_safe_filename]` `safe_filename(value, limit=200, location="attachment_name")` returns an attachment name as one line of visible text, with hidden, bidi (right-to-left override) and control characters removed and counted. `value` (any object; a non-string is empty), `limit` (int, default 200 characters) and `location` (str, default `attachment_name`). Calls `safe_header` with the file-name label, so the result is a `Result` whose `text` is one line with hidden, bidi and control characters removed (counted in `removed` and findings) and the phrase scan applied. Pure. The name is only ever shown as text; the attachment itself is never opened. **Tests:** `TestUntrustedTextMailAndAiHelpers.test_01_html_detection_headers_filenames_and_findings_json`.

- `[@ANCHOR: hams_helpdesk:untrusted_safe_header]` `safe_header(value, limit=200, location="header")` returns a `scan_text` result for a header-like value (Subject, a display name) with newlines turned into spaces. `value` (any object; a non-string is treated as empty), `limit` (int, default 200) and `location` (str, default `header`). Runs `scan_text`, then replaces newlines in `result.text` with spaces so a header cannot carry a second line. Returns the `Result`, with findings, `removed` and `score` from the scan. Pure and never raises. A header value is never rewritten into a different address; this is for text a person reads. **Tests:** `TestUntrustedTextMailAndAiHelpers.test_01_html_detection_headers_filenames_and_findings_json`.

- `[@ANCHOR: hams_helpdesk:untrusted_sanitize_any]` `sanitize_any(value, limit=None, location="text")` sends HTML to `sanitize_html` and anything else to `scan_text`. `value` is any object, `limit` an optional character cap for the plain text, `location` the finding label (default `text`). `looks_like_html` decides: HTML goes to `sanitize_html`, everything else to `scan_text`. Returns a `Result` either way. Pure; this is the entry point the ticket mixin uses for subject, description and message bodies, so it cannot fail on odd input. **Tests:** `TestUntrustedTextMailAndAiHelpers.test_01_html_detection_headers_filenames_and_findings_json`.

- `[@ANCHOR: hams_helpdesk:untrusted_sanitize_html]` `sanitize_html(value, limit=None, location="html")` runs the strict allow-list sanitizer and returns a `Result`: `text` is safe HTML, `plain` the visible text, findings say what was hidden or removed and how much (`hidden_html_*`, `hidden_attribute_*`, `visible_text_shrank`); a parse failure is an `unparseable_html` finding, `limit` cuts the plain text, then the phrase and encoding scans run. A non-string is treated as empty. **Tests:** `TestUntrustedTextHtmlSanitizer.test_01_sanitizer_methods_keep_allowed_tags_and_drop_everything_else`.

- `[@ANCHOR: hams_helpdesk:untrusted_scan_concatenation]` `scan_concatenation(parts, location="all_fields")` scans the non-empty fields joined with spaces and returns `split_`-prefixed phrase findings, so an instruction divided across fields is caught. `parts` is an iterable of strings (empty and None entries skipped; newlines in each become spaces) and `location` a str, default `all_fields`. The parts are joined with single spaces and passed to `_scan_phrases` with the prefix `split_`, so an instruction cut in two across subject and description is found. Returns a list of findings (possibly empty), never a `Result`; nothing is removed. Pure. **Tests:** `TestUntrustedTextBuildingBlocks.test_05_phrase_and_encoding_scans_look_at_decoded_text_without_returning_it`.

- `[@ANCHOR: hams_helpdesk:untrusted_scan_encodings]` `scan_encodings(text, location, findings)` adds an `encoded_instruction_<kind>` finding (weight at least 5) when text reads as an instruction only after decoding; the decoded text is never returned or stored beyond a short excerpt of the match. **Tests:** `TestUntrustedTextBuildingBlocks.test_05_phrase_and_encoding_scans_look_at_decoded_text_without_returning_it`.

- `[@ANCHOR: hams_helpdesk:untrusted_scan_text]` `scan_text(value, limit=None, location="text")` filters plain text and returns a `Result`: invisible characters stripped, blank-line padding collapsed (a `padding` finding), mixed-script words flagged, the text cut to `limit` (a `truncated` finding), then phrase and encoding scans; a non-string is treated as empty. **Tests:** `TestUntrustedTextBuildingBlocks.test_06_scan_text_clean_text_and_merge`.

- `[@ANCHOR: hams_helpdesk:untrusted_strip_invisible]` `strip_invisible(value, location="text", findings=None)` returns `(clean str, findings)`: invisible, control, format, tag, selector, bidi and filler characters are removed and counted, tag characters are decoded into an `unicode_tag_smuggling` finding, more than three combining marks in a row are cut, carriage returns are dropped, line separators become newlines and exotic spaces become a plain space; a non-string gives an empty string. **Tests:** `TestUntrustedTextBuildingBlocks.test_04_strip_invisible_removes_decodes_and_counts`.

- `[@ANCHOR: hams_helpdesk:untrusted_wrap_untrusted]` `wrap_untrusted(text, label="ticket")` returns the text inside a block delimited by a random per-call token that does not occur in the text, with the fixed statement before and after saying the block is data and not an instruction. **Tests:** `TestUntrustedTextMailAndAiHelpers.test_03_prompt_block_and_ai_output_filter`.

## Where stranger-written text is filtered: the ticket mixin and the mail gateway

Every path that creates or edits a ticket or posts a ticket message ends in `create()`, `write()` or `message_post()` on `hams_helpdesk.untrusted.mixin` (`models/untrusted_mixin.py`), which the ticket model inherits. Inbound mail is first inspected where every mail passes, `mail.thread.message_parse()` (`models/mail_thread_untrusted.py`), and the findings are handed to the mixin through a per-cursor stash. The filter itself is described above (`models/untrusted_text.py`); the policy is `docs/security/TICKET_PROMPT_INJECTION.md`. Tests are in `tests/test_ticket_untrusted_filter.py` and `tests/test_inbound_mail_inspection.py`.

### The mixin's overridable and internal methods

- `[@ANCHOR: hams_helpdesk:untrusted_field_specs]` **Purpose:** `_untrusted_field_specs()` (`@api.model`) returns the table of ticket fields that carry text from outside: field name to `(maximum characters, "html" or "text")`. It is a copy of `UNTRUSTED_FIELDS` (name 300, description 50000 html, callsign 40, the two NCMEC contact fields), so a module that adds its own free-text field overrides this method and adds to the returned dict without changing the shared table. **Interface:** no arguments; returns a new dict each call. No errors, no side effects. **Tests:** `TestTicketUntrustedFilter.test_16_the_mixin_helpers_by_themselves`.

- `[@ANCHOR: hams_helpdesk:untrusted_filter_vals]` **Purpose:** `_untrusted_filter_vals(vals)` (`@api.model`) is the one place a values dict is cleaned before it reaches the database. **Interface:** `vals` is the dict given to `create()` or `write()`, not modified (a copy is returned). Returns `(new_vals, results, raw)`: the cleaned dict, a list of `(field, Result)` for every present non-empty string field in the specs, and `{field: original text}` (cut to `RAW_EVIDENCE_LIMIT`, 100000 characters) for each field the filter changed. `html` fields go through `sanitize_html` and come back as `Markup`; `text` fields through `scan_text`, and a line break in `name` or `callsign` becomes a space. The keys of `SUSPICION_FIELDS` (`suspicion_score`, `suspicious`, `suspicion_removed`, `suspicion_log_ids`) are removed, so no caller can set them. **Special concerns:** pure apart from reading the specs; a value that is empty or not a string is passed through untouched. **Tests:** `TestTicketUntrustedFilter.test_16_the_mixin_helpers_by_themselves`.

- `[@ANCHOR: hams_helpdesk:untrusted_summary]` **Purpose:** `_untrusted_summary(results, extra_parts=())` (`@api.model`) combines the per-field results into the one verdict stored on the ticket. **Interface:** `results` is the list of `(field, Result)` from `_untrusted_filter_vals`; `extra_parts` is more plain text to scan together with the fields (the message body on a post). Returns `(findings, removed, score)`: the findings of every field plus those of `scan_concatenation` (an instruction split across fields), the characters removed, and the summed score with the weight of each `split_` finding added, capped at 100. **Tests:** `TestTicketUntrustedFilter.test_16_the_mixin_helpers_by_themselves`.

- `[@ANCHOR: hams_helpdesk:untrusted_record]` **Purpose:** `_untrusted_record(findings, removed, score, raw, source)` stores what the filter found on the tickets in `self`. **Interface:** the arguments are the outputs of `_untrusted_summary` and `_untrusted_filter_vals`; `source` is `"write"` or `"message"`. It does nothing when `self` is empty or when there are no findings, nothing removed and no raw original. For each ticket it creates one `hams_helpdesk.ticket.suspicion` log (findings JSON, raw original as JSON capped at twice `RAW_EVIDENCE_LIMIT`), adds `removed` to `suspicion_removed`, raises `suspicion_score` to the larger of the old and new score, and sets `suspicious` when that score reaches `SUSPICION_THRESHOLD`. **Special concerns:** runs as the helpdesk service account (`hams_helpdesk.user_helpdesk_service`, no `sudo()`), so staff can read the log and a portal customer cannot; the ticket write carries `untrusted_filter_bypass` so the filter does not run on its own bookkeeping. **Tests:** `TestTicketUntrustedFilter.test_16_the_mixin_helpers_by_themselves`.

- `[@ANCHOR: hams_helpdesk:untrusted_log_only]` **Purpose:** `_untrusted_log_only(findings, removed, score, raw, source)` is `_untrusted_record` for a ticket that `create()` just made: the counters were stored with the record, so only the log row is written. **Interface:** one ticket (`self.id` is used); the same arguments as `_untrusted_record`; nothing is written when there are no findings, nothing removed and no raw original. Runs as the helpdesk service account. **Tests:** `TestTicketUntrustedFilter.test_16_the_mixin_helpers_by_themselves`.

- `[@ANCHOR: hams_helpdesk:untrusted_attachment_count]` **Purpose:** `_untrusted_attachment_count()` counts the attachments of one ticket for the notice every AI read states: attachments are counted and never opened. **Interface:** `self.ensure_one()` (a recordset of any other size raises `ValueError`); returns the number of `ir.attachment` rows whose `res_model` and `res_id` are the ticket, or `None` when the count cannot be made (the failure is logged at info level, and `safe_view` then uses `ATTACHMENT_NOTICE_UNKNOWN`). Counted as the helpdesk service account. **Tests:** `TestTicketUntrustedFilter.test_17_attachments_are_counted_never_opened`, `TestTicketUntrustedFilter.test_08_safe_view`.

### The three entry points and the safe view

- `[@ANCHOR: hams_helpdesk:untrusted_create]` **Purpose:** `create(vals_list)` (`@api.model_create_multi`) filters every new ticket. **Interface:** a list of values dicts, as for any `create`; returns the new records. Each dict goes through `_untrusted_filter_vals` and `_untrusted_summary`; the score, the removed count and `suspicious` (set when the score reaches `SUSPICION_THRESHOLD`, 5) are stored with the record, and `_untrusted_log_only` writes the evidence row with source `"create"`. A caller's own `suspicion_*` values are discarded. **Tests:** `TestTicketUntrustedFilter.test_01_hidden_html_is_removed_flagged_and_original_kept`.

- `[@ANCHOR: hams_helpdesk:untrusted_write]` **Purpose:** `write(vals)` filters an edit of a ticket. **Interface:** `vals` as for `write`; returns the result of the parent write. It passes straight through when the context has `untrusted_filter_bypass` (the mixin's own bookkeeping) or when `vals` names no filtered field and no suspicion field; otherwise it filters, writes, and records the findings with source `"write"`. A caller cannot set the suspicion fields. **Tests:** `TestTicketUntrustedFilter.test_05_write_path_is_filtered`.

- `[@ANCHOR: hams_helpdesk:untrusted_message_post]` **Purpose:** `message_post(**kwargs)` filters a message posted on a ticket. **Interface:** the usual `message_post` keyword arguments; returns the new message. The body (up to `MESSAGE_BODY_LIMIT`, 50000 characters) goes through `sanitize_any` (HTML stays `Markup`, plain text stays text), `subject`, `email_from` and `reply_to` through `safe_header` (300 characters), and each attachment name through `safe_filename`; the attachment content is never read. A message with only a subject is still filtered. When the kwargs carry a `message_id`, the findings the mail gateway parked for that mail (`stash_for`) are popped and merged in (up to `MAX_FINDINGS`; the larger score; the removed counts added). Everything found is recorded with source `"message"`; the original is kept in the log when text changed. **Special concerns:** nothing is filtered when the context has `untrusted_filter_bypass`. **Tests:** `TestTicketUntrustedFilter.test_06_message_post_is_filtered`.

- `[@ANCHOR: hams_helpdesk:untrusted_safe_view]` **Purpose:** `safe_view(max_chars=0)` is the only way an AI reader (the MCP tools, the triage agent) is given a ticket: no AI reads a raw body or raw metadata. **Interface:** `max_chars` is 0 for no limit, or the number of characters of the body to keep (a longer body is cut and a note says how many characters there were). Returns one dict per ticket: `id`, `subject`, `callsign`, `description` (the visible text only), `untrusted_block` (the text wrapped in the untrusted-data markers), `attachments` and `attachments_notice`, `suspicious`, `suspicion_score`, `removed_or_flagged` and `findings` (only `vector`, `location` and `count`: the excerpt holds the very text that was removed, which staff read in the log). The ticket is filtered again on every call, so one stored before the filter existed is cleaned too, and the findings recorded at intake are included. **Tests:** `TestTicketUntrustedFilter.test_08_safe_view`, `TestTicketUntrustedFilter.test_09_safe_view_refilters_a_ticket_stored_before_the_filter`.

- `[@ANCHOR: hams_helpdesk:suspicion_findings_summary]` **Purpose:** `hams_helpdesk.ticket.suspicion.findings_summary` (`_compute_findings_summary`, depends on `findings_json`) is the one-line text staff see for a log row. **Interface:** a computed `Char` of `"<vector> x<count> in <location>"` entries joined with `"; "`, at most 500 characters; an empty or unreadable `findings_json` gives an empty string. The excerpts are not included. **Tests:** `TestTicketUntrustedFilter.test_01a_suspicion_record_name_is_the_source_and_ticket_never_the_text`.

### Inbound mail inspection (`models/mail_thread_untrusted.py`)

- `[@ANCHOR: hams_helpdesk:inbound_message_parse]` **Purpose:** `mail.thread.message_parse(message, save_original=False)` (`@api.model`, an override) runs the checks that need the whole parsed `email.message.Message` (header allow-list, text/plain against text/html, attachment names, archives, SES verdicts, bulk-mail signals), because Odoo's own parse keeps one alternative part and flattens the headers. **Interface:** the same arguments as Odoo's; returns Odoo's `msg_dict` unchanged. As a side effect it parks `{"findings", "score", "removed", "ses_verdicts", "mail_signals"}` in the cursor stash under the mail's Message-ID (an empty string when there is none). **Special concerns:** never raises on a hostile mail (`inspect_mail` records an error as a finding); the extra data is not a key of `msg_dict`, because Odoo passes every key of it on to `mail.message.create()` and refuses an unknown one; `mail_signals["reply_headers"]` is true only for a reply to a message already stored on a ticket or pager incident. The stash is read by `message_post()` and by `message_new()`. **Tests:** `TestMailGatewayInspection.test_04_message_parse_parks_the_findings_under_the_message_id`, `TestMailGatewayInspection.test_01_a_mismatched_alternative_and_a_disguised_attachment_mark_the_ticket`.

- `[@ANCHOR: hams_helpdesk:inbound_stash_for]` **Purpose:** `stash_for(env)` returns the dict that holds the findings of mail parsed in this transaction. **Interface:** `env` is any Odoo environment; returns the dict stored on the database cursor (`env.cr.cache`, key `MAIL_STASH_KEY`), created empty on first use; the same dict is returned every time for one cursor. **Special concerns:** the cursor cache is cleared on commit and rollback, so nothing outlives the transaction that parsed the mail; the reader pops its entry. **Tests:** `TestMailGatewayInspection.test_03_the_parked_findings_do_not_outlive_the_message`, `TestMailGatewayInspection.test_04_message_parse_parks_the_findings_under_the_message_id`.

## The inbound spam filter's helpers (`models/inbound_spam_filter.py`)

Pure functions (no ORM, no network), used by `inbound_spam_reasons()` when a ticket is created from mail; see `docs/stories/helpdesk_ticket_creation.md`. Tests are in `tests/test_inbound_spam_filter.py`.

- `[@ANCHOR: hams_helpdesk:inbound_detect_spam_signals]` **Purpose:** `detect_inbound_spam_signals(subject, body_html)` finds the known spam and phishing patterns in one message. **Interface:** both arguments are strings or `None`. Returns a list of plain-English reasons, empty when nothing matched, one per signal: a `SPAM_SUBJECT_PATTERNS` match in the subject, an unsolicited convention hotel-booking offer to exhibitors, and, only when the body has at least one link, a mentioned brand of `KNOWN_BRAND_DOMAINS` whose links do not all belong to that brand's own domains. The sender's address is deliberately not an argument: a genuine mail about a brand often comes from a third-party address, so only a mismatched link counts. **Special concerns:** never raises on hostile input (a malformed link contributes no domain). **Tests:** `TestInboundSpamFilter.test_01_domain_expiring_spam_is_flagged` and the cases after it up to `test_16`, `test_31_a_convention_hotel_booking_offer_to_exhibitors_is_flagged`.

- `[@ANCHOR: hams_helpdesk:inbound_registrable_domain]` **Purpose:** `_registrable_domain(hostname)` normalizes a host name for the brand comparison. **Interface:** a string or `None`; returns it stripped, lower-cased, with one leading `www.` removed, or `""` for an empty value. It is not a public-suffix lookup: every `KNOWN_BRAND_DOMAINS` entry is already the brand's own registrable domain, so a suffix match is enough. **Tests:** `TestInboundSpamFilter.test_16b_the_link_and_brand_helpers_by_themselves`.

- `[@ANCHOR: hams_helpdesk:inbound_domain_matches_brand]` **Purpose:** `_domain_matches_brand(hostname, real_domains)` says whether a link's host belongs to a brand. **Interface:** `hostname` a host name; `real_domains` a tuple of the brand's own domains. True when the normalized host equals one of them or ends with `"." + domain` (`accounts.intuit.com`), false for `notintuit.com`, `intuit.com.evil.example` and an empty host. **Tests:** `TestInboundSpamFilter.test_16b_the_link_and_brand_helpers_by_themselves`.

- `[@ANCHOR: hams_helpdesk:inbound_extract_link_domains]` **Purpose:** `_extract_link_domains(body_html)` lists the hosts a message links to. **Interface:** a string or `None`; returns a set of lower-cased host names found in `href="..."` or `href='...'` values. A relative link has no host and a malformed one (`http://[x/`, which makes `urlsplit` raise `ValueError`) is skipped, so one bad link never stops a ticket from being created. **Tests:** `TestInboundSpamFilter.test_16b_the_link_and_brand_helpers_by_themselves`, `TestInboundSpamFilter.test_11_malformed_href_does_not_raise_and_other_links_still_count`.

- `[@ANCHOR: hams_helpdesk:inbound_brand_mentioned]` **Purpose:** `_brand_mentioned(brand, text)` says whether a message names a brand. **Interface:** `brand` a lower-case name from `KNOWN_BRAND_DOMAINS`; `text` a string; returns True when the name occurs as a whole word, ignoring case (`paypalish` does not count). **Tests:** `TestInboundSpamFilter.test_16b_the_link_and_brand_helpers_by_themselves`.

- `[@ANCHOR: hams_helpdesk:ses_verdicts]` **Purpose:** `ses_verdicts(message)` reads the Amazon SES scan verdicts that SES writes into the stored message (`X-SES-Spam-Verdict`, `X-SES-Virus-Verdict`; hams.com's receipt rule has `ScanEnabled`). **Interface:** `message` an `email.message.Message`; returns a dict such as `{"spam": "FAIL", "virus": "PASS"}` with values stripped, upper-cased and cut to 32 characters (a sender controls any header SES did not write); a header that is absent is absent from the dict. Only a `FAIL` is acted on (`ses_verdict_reasons`); `GRAY` and `PROCESSING_FAILED` mean SES could not decide, and SPF, DKIM and DMARC results are weak bulk signals only. **Tests:** `TestInboundSpamFilter.test_17_ses_verdicts_flag_only_a_fail`, `TestInboundSpamFilter.test_18_a_message_without_ses_headers_has_no_verdicts`, `TestInboundSpamFilter.test_19_a_hostile_verdict_value_is_bounded`.

- `[@ANCHOR: hams_helpdesk:inbound_ses_verdict_reasons]` **Purpose:** `ses_verdict_reasons(verdicts)` turns the verdict dict into reasons. **Interface:** `verdicts` is the dict of `ses_verdicts()` or `None`; returns `"Amazon SES spam scan verdict: FAIL"` and/or `"Amazon SES virus scan verdict: FAIL"`, and an empty list for every other value. **Tests:** `TestInboundSpamFilter.test_17_ses_verdicts_flag_only_a_fail`, `TestInboundSpamFilter.test_18_a_message_without_ses_headers_has_no_verdicts`.
## Plumbing functions (ADR 0090, 0113)

Entries for the helper functions that have no user-visible story of their own. Each says what the function is for, its call interface and its errors.

### The ticket-text AI-reader rule (`ai_reader_rule.py`, pure Python, no Odoo import)

- `[@ANCHOR: hams_helpdesk:ai_reader_check_source]` `check_source(source, filename="<src>")` is the rule itself for one Python source text. **Purpose:** find code that hands raw ticket text to an AI reader. **Arguments:** `source` (str, the file text), `filename` (str, only for messages). **Returns:** a list of `(line, message)` tuples, empty when the source is clean or does not parse (a `SyntaxError` gives `[]`, never an exception). It checks two shapes: an `mcp_*` method on a ticket-like model that reads a free-text attribute without a shared-filter marker, and a daemon `execute`/`execute_kw`/`call` of `read`/`search_read`/`search_fetch` on a ticket-like model that asks for text fields (or all fields) in a function with no safe-view marker. Pure and side-effect free. **Tests:** `test_02_a_raw_mcp_method_on_a_ticket_model_is_caught` and its neighbours in `tests/test_ai_reader_sites.py`.

- `[@ANCHOR: hams_helpdesk:ai_reader_constants]` `_constants(tree)` takes a parsed module (`ast.Module`) and returns `(consts, value)`: `consts` maps each module-level `NAME = <literal>` to its Python value (strings, lists and tuples of them, and `list + list` sums, resolving earlier names), and `value(node)` resolves any expression node the same way, returning None when it cannot. It is how `check_source` reads `client.execute(MODEL, "read", ..., _TICKET_FIELDS + ["partner_id"])`. Never raises on odd nodes. **Tests:** `test_04_a_raw_daemon_read_is_caught_even_through_constants`.

- `[@ANCHOR: hams_helpdesk:ai_reader_markers]` `_markers(node)` returns the set of every identifier, attribute name and string constant under an AST node. `check_source` intersects it with `SAFE_MARKERS` to decide whether a function goes through the safe view (a string counts because a daemon names the method as text, `"mcp_safe_read"`). **Tests:** `test_03_the_same_method_through_the_filter_passes`.

- `[@ANCHOR: hams_helpdesk:ai_reader_ignored]` `_ignored(lines, *linenos)` takes the source split into lines and one or more 1-based line numbers; it returns True when any of those lines (those out of range are skipped) holds the `# ticket-ai-ignore: <reason>` tag, which silences one finding. **Tests:** `test_08_the_ignore_tag_with_a_reason_silences_one_call`.

- `[@ANCHOR: hams_helpdesk:ai_reader_py_files]` `_py_files(root, daemon_only=False)` is a generator of the `.py` file paths the rule scans under `root`: the files of `_candidate_files`, minus `test_*` files, and with `daemon_only=True` only those with a `daemon` or `daemons` directory in their path. **Tests:** `test_10_only_tracked_files_are_scanned_so_an_untracked_stray_directory_cannot_fail_it`.

### Beacon and webhook controllers

- `[@ANCHOR: hams_helpdesk:browser_answer]` `_answer(status)` builds the only reply the browser-error beacon (`POST /hams_helpdesk/browser_exception`) gives: an empty body with the given integer HTTP status and `Cache-Control: no-store`, so no cache keeps a verdict and a prober learns nothing from a body. **Tests:** `TestBrowserExceptionEndpoint`.

- `[@ANCHOR: hams_helpdesk:browser_report_rejected]` `ReportRejected(status, reason)` is the exception the report validators raise: `status` (int) is the HTTP status to answer, `reason` (short fixed word, never a piece of the report) is for the log and tests; the constructor stores both as attributes and uses `reason` as the exception message. **Tests:** `TestBrowserReportSchema`.

- `[@ANCHOR: hams_helpdesk:github_verify_signature]` `_verify_github_signature(raw_body, secret, signature_header)` checks GitHub's `X-Hub-Signature-256`. **Arguments:** `raw_body` (bytes, the exact unparsed request body), `secret` (str, the shared webhook secret), `signature_header` (str or None, `sha256=<hex>`). **Returns:** True only when the header is the HMAC-SHA256 of the body under the secret; False, never an exception, for a missing secret or header, a wrong prefix, an HMAC error or a non-ASCII header. The comparison is constant time. **Tests:** `test_github_webhook_valid_signature_is_accepted`, `test_github_webhook_non_ascii_signature_is_denied_not_an_error`.

### Daemon reporter (`daemon/exception_reporter.py`, standard library only)

- `[@ANCHOR: hams_helpdesk:daemon_main]` `main(argv=None)` is the command line the `hams-exception-report@.service` unit runs: `exception_reporter.py report-unit <unit>`. It calls `report_unit` and prints the answer. **Returns** the exit code: 0 delivered, 1 when delivery failed (`ReportError`, message on stderr, no URL or key), 2 for any other argument shape (usage on stderr). **Tests:** `MainTests` in `daemon/test_exception_reporter.py`.

- `[@ANCHOR: hams_helpdesk:daemon_reporting_failures_init]` `reporting_failures(daemon_name, **options)` stores the daemon's unit name and the options (`url`, `database`, `key_file`, `timeout`) that are later passed to `report_exception`. It does no I/O. `daemon_name` (str) is the systemd unit name the report is filed under, for example `ses-inbound-mail-ingest.service`; `**options` are passed unchanged to `report_exception` on a failure. The class is a `contextlib.ContextDecorator`, so it works as `with reporting_failures(...)` or as a decorator. Returns the instance; nothing is validated until a failure is reported.

- `[@ANCHOR: hams_helpdesk:daemon_reporting_failures_enter]` `__enter__()` returns the guard itself and does nothing else. Takes no arguments, returns the guard itself so `with reporting_failures(name) as guard` binds it, and acquires nothing: no lock, file or network connection is opened on entry. All the work happens in `__exit__`, so entering the block can never fail or slow the daemon's start.

- `[@ANCHOR: hams_helpdesk:daemon_reporting_failures_exit]` `__exit__(exc_type, exc, tb)` reports the exception through `report_exception` only when it is an `Exception` (a `SystemExit` or `KeyboardInterrupt` is a normal stop and is not reported) and always returns False, so the daemon's own exception propagates unchanged. A failed report never raises into it. **Tests:** `ReportingFailuresTests` in `daemon/test_exception_reporter.py`, `test_reporting_failures_still_raises_the_daemons_own_exception_when_the_key_is_unreadable`.

### Module install hook

- `[@ANCHOR: hams_helpdesk:post_init_hook]` `post_init_hook(env)` runs once when the module is first installed and calls `register_exception_reporter_key(env)` so the Daemon Key Manager issues the reporter key; installed databases get the same through `migrations/1.1`. It takes the install environment and returns nothing. **Tests:** `test_the_post_init_hook_registers_the_key_on_a_fresh_install`.

### Request filters (`models/ir_http_exception.py`)

- `[@ANCHOR: hams_helpdesk:http_contains_nul]` `_contains_nul(value)` returns True when a NUL byte appears in any string or bytes inside `value`, walking dicts (keys and values), lists, tuples and sets iteratively (no recursion limit). It exists because PostgreSQL cannot store or compare a NUL and the handler used to answer 500. Pure. **Tests:** `test_a_nul_inside_nested_values_is_found_and_clean_values_are_not`.

- `[@ANCHOR: hams_helpdesk:http_missing_required_params]` `_missing_required_params(endpoint)` returns the names of parameters the handler requires (no default, positional-or-keyword or keyword-only, after `self`) that `request.params` does not carry. It returns `[]` for a route type other than http, json or jsonrpc, a handler Odoo gives no function for, or a signature that cannot be read. The dispatcher answers 400 naming them. **Tests:** `test_a_bare_get_of_each_qa_014_route_is_a_400_not_a_500`.

### Triage wake-up spool

- `[@ANCHOR: hams_helpdesk:triage_spool_dir]` `spool_dir()` returns the directory the wake-up files go in: the `HAMS_TRIAGE_SPOOL_DIR` environment variable when set and non-empty, else `/opt/hams/spool/ticket_triage`. It reads the environment on every call and creates nothing. Takes no arguments and returns a `str`. It reads the environment variable named by `SPOOL_DIR_ENV` (`HAMS_TRIAGE_SPOOL_DIR`) on every call, so a test can set it per run. A non-empty value wins, otherwise `DEFAULT_SPOOL_DIR`. It creates and checks nothing: the directory's existence and permissions are the concern of `write_triage_wakeups`, which never raises. **Tests:** `test_13a_spool_dir_is_the_environment_override_else_the_default`.

- `[@ANCHOR: hams_helpdesk:ticket_schedule_triage_wakeup]` `hams_helpdesk.ticket._schedule_triage_wakeup(ticket_ids)` (model method) adds a post-commit callback that calls `write_triage_wakeups(ids)` for the ids, so the spool is written only after the create's transaction has committed. An empty list does nothing; any failure to schedule is logged and swallowed, never raised into `create()`. **Tests:** `test_04_a_failure_to_even_schedule_it_never_breaks_ticket_creation`.

- `[@ANCHOR: hams_helpdesk:ticket_silent_exception_note]` `hams_helpdesk.ticket._silent_exception_note()` returns the sentence appended to the assignment note of an exception ticket that pages after a quiet stretch: an empty string when the context key `hams_exception_silent_count` is missing or zero, else a leading space and the translated count of other exception tickets opened without paging. **Tests:** `test_silent_exception_note_is_empty_or_names_the_unpaged_count`.

