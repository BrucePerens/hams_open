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

## The untrusted-text filter (`models/untrusted_text.py`)

Pure Python (no Odoo import); the policy and the list of vectors are in `docs/TICKET_PROMPT_INJECTION.md`, the hostile corpus in `tests/test_untrusted_text.py` and the direct tests below in `tests/test_untrusted_text_api.py`.

- `[@ANCHOR: hams_helpdesk:untrusted_result_init]` `Result(text="", plain=None, findings=None, removed=0, score=0)` holds what the filter found: the clean `text` (or sanitized HTML), its visible `plain` text (defaults to `text`), the `findings` list (each result gets its own list), the number of characters `removed` and the suspicion `score`. **Tests:** `TestUntrustedTextBuildingBlocks.test_01_result_holds_text_findings_and_a_suspicion_flag`.
- `[@ANCHOR: hams_helpdesk:untrusted_result_iter]` `iter(Result)` yields `(text, removed)`, the tuple `clean_untrusted_text` always returned, so older callers can still unpack a result. **Tests:** `TestUntrustedTextBuildingBlocks.test_01_result_holds_text_findings_and_a_suspicion_flag`.
- `[@ANCHOR: hams_helpdesk:untrusted_result_suspicious]` `Result.suspicious` is True when `score` is at or over `SUSPICION_THRESHOLD` (5); a suspicious item is never auto-triaged, auto-answered or acted on by an AI. **Tests:** `TestUntrustedTextBuildingBlocks.test_01_result_holds_text_findings_and_a_suspicion_flag`.
- `[@ANCHOR: hams_helpdesk:untrusted_sanitizer_init]` `_Sanitizer(location)` is the allow-list HTML parser behind `sanitize_html`; it starts with white as the background colour and empty output, plain-text, dropped-text and finding lists. **Tests:** `TestUntrustedTextHtmlSanitizer.test_01_sanitizer_methods_keep_allowed_tags_and_drop_everything_else`.
- `[@ANCHOR: hams_helpdesk:untrusted_sanitizer_attr_texts]` `_Sanitizer._attr_texts(tag, attrs)` records text an attacker could hide in attributes: CSS `content:`, alt, title, aria and `data-` values of four or more characters, `data:` sources, event handlers and `javascript:`/`vbscript:`/`data:` links. **Tests:** `TestUntrustedTextHtmlSanitizer.test_02_comments_declarations_instructions_and_attributes_are_findings`.
- `[@ANCHOR: hams_helpdesk:untrusted_sanitizer_note_drop]` `_Sanitizer._note_drop(reason, text)` records stripped, non-empty text under its reason and counts its length; blank text is ignored. **Tests:** `TestUntrustedTextHtmlSanitizer.test_02_comments_declarations_instructions_and_attributes_are_findings`.
- `[@ANCHOR: hams_helpdesk:untrusted_sanitizer_handle_comment]` `_Sanitizer.handle_comment(data)` removes an HTML comment and records its text as hidden. **Tests:** `TestUntrustedTextHtmlSanitizer.test_02_comments_declarations_instructions_and_attributes_are_findings`.
- `[@ANCHOR: hams_helpdesk:untrusted_sanitizer_handle_data]` `_Sanitizer.handle_data(data)` keeps and escapes visible text, and counts text inside a dropped element as hidden under its reason. **Tests:** `TestUntrustedTextHtmlSanitizer.test_01_sanitizer_methods_keep_allowed_tags_and_drop_everything_else`.
- `[@ANCHOR: hams_helpdesk:untrusted_sanitizer_handle_endtag]` `_Sanitizer.handle_endtag(tag)` closes the nearest open element of that name (an unmatched end tag is ignored), restores the background, and for a link flags text that names a different host than the `href` (`link_text_href_mismatch`, weight 3). **Tests:** `TestUntrustedTextHtmlSanitizer.test_01_sanitizer_methods_keep_allowed_tags_and_drop_everything_else`.
- `[@ANCHOR: hams_helpdesk:untrusted_sanitizer_handle_pi]` `_Sanitizer.handle_pi(data)` removes a processing instruction and records its text as hidden. **Tests:** `TestUntrustedTextHtmlSanitizer.test_02_comments_declarations_instructions_and_attributes_are_findings`.
- `[@ANCHOR: hams_helpdesk:untrusted_sanitizer_handle_startendtag]` `_Sanitizer.handle_startendtag(tag, attrs)` treats `<tag/>` as a start tag followed at once by its end tag (void tags have no end). **Tests:** `TestUntrustedTextHtmlSanitizer.test_01_sanitizer_methods_keep_allowed_tags_and_drop_everything_else`.
- `[@ANCHOR: hams_helpdesk:untrusted_sanitizer_handle_starttag]` `_Sanitizer.handle_starttag(tag, attrs)` drops an element and its subtree when its tag is never shown or `_hidden_reason` finds it hidden, tracks the background colour, and emits only allow-listed tags with no attributes except a checked `a href` carrying `rel="nofollow noopener noreferrer"`. **Tests:** `TestUntrustedTextHtmlSanitizer.test_01_sanitizer_methods_keep_allowed_tags_and_drop_everything_else`.
- `[@ANCHOR: hams_helpdesk:untrusted_sanitizer_unknown_decl]` `_Sanitizer.unknown_decl(data)` removes a CDATA block or other declaration and records its text as hidden. **Tests:** `TestUntrustedTextHtmlSanitizer.test_02_comments_declarations_instructions_and_attributes_are_findings`.
- `[@ANCHOR: hams_helpdesk:untrusted_add]` `_add(findings, vector, location, count, excerpt="", weight=0)` appends one finding dict (vector, location, count, excerpt, weight); a zero count with no weight is not a finding, and nothing is added once `MAX_FINDINGS` (40) are held. **Tests:** `TestUntrustedTextBuildingBlocks.test_02_findings_are_excerpted_capped_and_scored`.
- `[@ANCHOR: hams_helpdesk:untrusted_bucket]` `_bucket(ch)` names the finding vector for a character that must be removed (line separator, Unicode tag, variation selector, bidi control, invisible filler, zero width, control, format, private use, unassigned or surrogate) and returns None for a character to keep (newline and tab included). **Tests:** `TestUntrustedTextBuildingBlocks.test_03_characters_are_bucketed_and_scripts_told_apart`.
- `[@ANCHOR: hams_helpdesk:untrusted_contrast]` `_contrast(a, b)` returns the WCAG contrast ratio (1.0 to 21.0) between two RGB tuples; below 1.6 the text counts as unreadable. **Tests:** `TestUntrustedTextBuildingBlocks.test_07_css_colour_and_size_helpers`.
- `[@ANCHOR: hams_helpdesk:untrusted_decoded_variants]` `_decoded_variants(text)` returns `(kind, decoded text)` pairs a reader could be asked to decode: base64, hex, percent, HTML entities, quoted-printable, and always rot13, reversed and leetspeak; for scanning only. **Tests:** `TestUntrustedTextBuildingBlocks.test_05_phrase_and_encoding_scans_look_at_decoded_text_without_returning_it`.
- `[@ANCHOR: hams_helpdesk:untrusted_excerpt]` `_excerpt(value)` returns a short printable excerpt, safe for a staff banner or a log: characters that are not printable or are one of `<>&"'` become `?`, whitespace runs collapse, and it is cut to `EXCERPT_CHARS` (60) with `...`. **Tests:** `TestUntrustedTextBuildingBlocks.test_02_findings_are_excerpted_capped_and_scored`.
- `[@ANCHOR: hams_helpdesk:untrusted_hidden_reason]` `_hidden_reason(tag, attrs, bg)` returns why an element's text would not be visible (hidden attribute, aria-hidden, closed details, hidden input, display none, visibility, opacity, tiny font, zero box with overflow, clipping, off-screen offset or indent, scale 0, zero line height, transparent or low-contrast colour against the background `bg`), or None when it would be shown. **Tests:** `TestUntrustedTextBuildingBlocks.test_08_hidden_reasons_background_and_links`.
- `[@ANCHOR: hams_helpdesk:untrusted_is_phrase]` `_is_phrase(vector)` is True for a heuristic phrase vector (one of the `_PHRASES` names, or any `split_` name) and False for hidden-content vectors; it decides how `_score` weighs a finding. **Tests:** `TestUntrustedTextBuildingBlocks.test_02_findings_are_excerpted_capped_and_scored`.
- `[@ANCHOR: hams_helpdesk:untrusted_luminance]` `_luminance(rgb)` returns the WCAG relative luminance (0.0 black to 1.0 white) of an RGB tuple. **Tests:** `TestUntrustedTextBuildingBlocks.test_07_css_colour_and_size_helpers`.
- `[@ANCHOR: hams_helpdesk:untrusted_mixed_script_words]` `_mixed_script_words(text)` returns the words that mix Latin letters with letters of another listed script, the homoglyph trick (`pаypal` with a Cyrillic a); a word wholly in one script is not returned. **Tests:** `TestUntrustedTextBuildingBlocks.test_03_characters_are_bucketed_and_scripts_told_apart`.
- `[@ANCHOR: hams_helpdesk:untrusted_new_bg]` `_new_bg(attrs, bg)` returns the background colour an element sets (style `background-color`, `background` or the `bgcolor` attribute) as an RGB tuple, or `bg` unchanged when it sets none or one that is not a plain colour. **Tests:** `TestUntrustedTextBuildingBlocks.test_08_hidden_reasons_background_and_links`.
- `[@ANCHOR: hams_helpdesk:untrusted_parse_color]` `_parse_color(value)` reads a CSS colour (named, `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb()`/`rgba()`, `hsl()` near white) as an RGB tuple, `"invisible"` for a nearly transparent one, or None when it is not a colour a reader can compare; a number that is not finite is None, never an exception. **Tests:** `TestUntrustedTextBuildingBlocks.test_07_css_colour_and_size_helpers`.
- `[@ANCHOR: hams_helpdesk:untrusted_part_text]` `_part_text(part)` decodes a mail part to text with its declared charset, replacing bad bytes, and falls back to UTF-8 for an unknown charset; a part with no payload is an empty string. **Tests:** `TestUntrustedTextMailAndAiHelpers.test_02_mime_alternatives_and_whole_mail_inspection`.
- `[@ANCHOR: hams_helpdesk:untrusted_safe_href]` `_safe_href(value)` returns the stripped link when its scheme is http or https with a host, or mailto, and None for every other scheme, a relative or host-less address and a value that does not parse. **Tests:** `TestUntrustedTextBuildingBlocks.test_08_hidden_reasons_background_and_links`.
- `[@ANCHOR: hams_helpdesk:untrusted_scan_phrases]` `_scan_phrases(text, location, findings, prefix="")` adds one finding per heuristic phrase that matches (name prefixed with `prefix`) and returns how many phrases matched. **Tests:** `TestUntrustedTextBuildingBlocks.test_05_phrase_and_encoding_scans_look_at_decoded_text_without_returning_it`.
- `[@ANCHOR: hams_helpdesk:untrusted_scan_views]` `_scan_views(text)` returns the strings a phrase scan looks at: the folded text, and the NFKC-only lower-cased text as well when it differs (for non-Latin phrases). **Tests:** `TestUntrustedTextBuildingBlocks.test_05_phrase_and_encoding_scans_look_at_decoded_text_without_returning_it`.
- `[@ANCHOR: hams_helpdesk:untrusted_score]` `_score(findings)` adds hidden-content weights in full; phrase findings are weak evidence, so on their own they total at most 4 unless one has weight 5 or more or two different phrase vectors are present. **Tests:** `TestUntrustedTextBuildingBlocks.test_02_findings_are_excerpted_capped_and_scored`.
- `[@ANCHOR: hams_helpdesk:untrusted_script]` `_script(ch)` returns the script family name (LATIN, CYRILLIC, GREEK, ARMENIAN, HEBREW, ARABIC, CHEROKEE, COPTIC) of a letter, or None for anything else or an unnamed character. **Tests:** `TestUntrustedTextBuildingBlocks.test_03_characters_are_bucketed_and_scripts_told_apart`.
- `[@ANCHOR: hams_helpdesk:untrusted_style_dict]` `_style_dict(style)` splits an inline style into a lower-cased property dict, keeping semicolons inside parentheses (`url(a;b)`) in the value; None gives an empty dict. **Tests:** `TestUntrustedTextBuildingBlocks.test_07_css_colour_and_size_helpers`.
- `[@ANCHOR: hams_helpdesk:untrusted_tiny_size]` `_tiny_size(value)` is True when a CSS font size is under 5 px after unit conversion (px, pt, em, rem, %, ex, ch, vw, vh, mm, cm, in) or is `xx-small` or `x-small`. **Tests:** `TestUntrustedTextBuildingBlocks.test_07_css_colour_and_size_helpers`.
- `[@ANCHOR: hams_helpdesk:untrusted_attachment_notice]` `attachment_notice(count)` returns the fixed sentence every AI read states: attachments are never read, N were not opened. **Tests:** `TestUntrustedTextMailAndAiHelpers.test_01_html_detection_headers_filenames_and_findings_json`.
- `[@ANCHOR: hams_helpdesk:untrusted_clean_untrusted_text]` `clean_untrusted_text(value, limit)` returns `(clean text, characters removed)`, the original simulated-band signature, through `scan_text`. **Tests:** `TestUntrustedTextBuildingBlocks.test_06_scan_text_clean_text_and_merge`.
- `[@ANCHOR: hams_helpdesk:untrusted_compare_alternatives]` `compare_alternatives(plain_text, html_text, location="alternative_parts")` returns a `mime_alternative_mismatch` finding (weight 4) when the text/plain and text/html parts of one mail differ by at least eight words and over 40 percent of the longer one, else an empty list. **Tests:** `TestUntrustedTextMailAndAiHelpers.test_02_mime_alternatives_and_whole_mail_inspection`.
- `[@ANCHOR: hams_helpdesk:untrusted_filter_ai_output]` `filter_ai_output(text, allowed_hosts=(), location="ai_output")` makes a model's output safe to store or render: invisible characters, markdown images, HTML tags and reference definitions removed, links and bare URLs kept only for an https address on an allowed host (others become their text or `[link removed]`), with an `ai_output_link_or_image_removed` finding when anything was removed. **Tests:** `TestUntrustedTextMailAndAiHelpers.test_03_prompt_block_and_ai_output_filter`.
- `[@ANCHOR: hams_helpdesk:untrusted_findings_from_json]` `findings_from_json(value)` returns the list of finding dicts in stored JSON, and `[]` for anything that is not JSON or not a list. **Tests:** `TestUntrustedTextMailAndAiHelpers.test_01_html_detection_headers_filenames_and_findings_json`.
- `[@ANCHOR: hams_helpdesk:untrusted_findings_to_json]` `findings_to_json(findings)` serialises at most `MAX_FINDINGS` findings as compact ASCII JSON. **Tests:** `TestUntrustedTextMailAndAiHelpers.test_01_html_detection_headers_filenames_and_findings_json`.
- `[@ANCHOR: hams_helpdesk:untrusted_fold_for_scanning]` `fold_for_scanning(text)` returns the NFKC form with look-alike Cyrillic and Greek letters mapped to Latin and case-folded; it is used only to look for instructions and never reaches a reader. **Tests:** `TestUntrustedTextBuildingBlocks.test_03_characters_are_bucketed_and_scripts_told_apart`.
- `[@ANCHOR: hams_helpdesk:untrusted_inspect_mail]` `inspect_mail(message, location="mail")` checks one parsed mail before anything reads it: header allow-list and free-text headers scanned, a repeated singleton header flagged, every text/plain and text/html pair compared, attachment names filtered, attachments counted and never opened, archives and nested messages flagged. It returns a `Result` with empty text; an error during inspection is itself a suspicious finding, so a mail is never dropped or passed as clean. **Tests:** `TestUntrustedTextMailAndAiHelpers.test_02_mime_alternatives_and_whole_mail_inspection`.
- `[@ANCHOR: hams_helpdesk:untrusted_looks_like_html]` `looks_like_html(value)` is True for a string containing a tag or a comment, and False for anything else, including a non-string. **Tests:** `TestUntrustedTextMailAndAiHelpers.test_01_html_detection_headers_filenames_and_findings_json`.
- `[@ANCHOR: hams_helpdesk:untrusted_merge]` `merge(results)` combines per-field results and returns `(findings capped at MAX_FINDINGS, characters removed summed, score summed and capped at 100)`. **Tests:** `TestUntrustedTextBuildingBlocks.test_06_scan_text_clean_text_and_merge`.
- `[@ANCHOR: hams_helpdesk:untrusted_safe_filename]` `safe_filename(value, limit=200, location="attachment_name")` returns an attachment name as one line of visible text, with hidden, bidi (right-to-left override) and control characters removed and counted. **Tests:** `TestUntrustedTextMailAndAiHelpers.test_01_html_detection_headers_filenames_and_findings_json`.
- `[@ANCHOR: hams_helpdesk:untrusted_safe_header]` `safe_header(value, limit=200, location="header")` returns a `scan_text` result for a header-like value (Subject, a display name) with newlines turned into spaces. **Tests:** `TestUntrustedTextMailAndAiHelpers.test_01_html_detection_headers_filenames_and_findings_json`.
- `[@ANCHOR: hams_helpdesk:untrusted_sanitize_any]` `sanitize_any(value, limit=None, location="text")` sends HTML to `sanitize_html` and anything else to `scan_text`. **Tests:** `TestUntrustedTextMailAndAiHelpers.test_01_html_detection_headers_filenames_and_findings_json`.
- `[@ANCHOR: hams_helpdesk:untrusted_sanitize_html]` `sanitize_html(value, limit=None, location="html")` runs the strict allow-list sanitizer and returns a `Result`: `text` is safe HTML, `plain` the visible text, findings say what was hidden or removed and how much (`hidden_html_*`, `hidden_attribute_*`, `visible_text_shrank`); a parse failure is an `unparseable_html` finding, `limit` cuts the plain text, then the phrase and encoding scans run. A non-string is treated as empty. **Tests:** `TestUntrustedTextHtmlSanitizer.test_01_sanitizer_methods_keep_allowed_tags_and_drop_everything_else`.
- `[@ANCHOR: hams_helpdesk:untrusted_scan_concatenation]` `scan_concatenation(parts, location="all_fields")` scans the non-empty fields joined with spaces and returns `split_`-prefixed phrase findings, so an instruction divided across fields is caught. **Tests:** `TestUntrustedTextBuildingBlocks.test_05_phrase_and_encoding_scans_look_at_decoded_text_without_returning_it`.
- `[@ANCHOR: hams_helpdesk:untrusted_scan_encodings]` `scan_encodings(text, location, findings)` adds an `encoded_instruction_<kind>` finding (weight at least 5) when text reads as an instruction only after decoding; the decoded text is never returned or stored beyond a short excerpt of the match. **Tests:** `TestUntrustedTextBuildingBlocks.test_05_phrase_and_encoding_scans_look_at_decoded_text_without_returning_it`.
- `[@ANCHOR: hams_helpdesk:untrusted_scan_text]` `scan_text(value, limit=None, location="text")` filters plain text and returns a `Result`: invisible characters stripped, blank-line padding collapsed (a `padding` finding), mixed-script words flagged, the text cut to `limit` (a `truncated` finding), then phrase and encoding scans; a non-string is treated as empty. **Tests:** `TestUntrustedTextBuildingBlocks.test_06_scan_text_clean_text_and_merge`.
- `[@ANCHOR: hams_helpdesk:untrusted_strip_invisible]` `strip_invisible(value, location="text", findings=None)` returns `(clean str, findings)`: invisible, control, format, tag, selector, bidi and filler characters are removed and counted, tag characters are decoded into an `unicode_tag_smuggling` finding, more than three combining marks in a row are cut, carriage returns are dropped, line separators become newlines and exotic spaces become a plain space; a non-string gives an empty string. **Tests:** `TestUntrustedTextBuildingBlocks.test_04_strip_invisible_removes_decodes_and_counts`.
- `[@ANCHOR: hams_helpdesk:untrusted_wrap_untrusted]` `wrap_untrusted(text, label="ticket")` returns the text inside a block delimited by a random per-call token that does not occur in the text, with the fixed statement before and after saying the block is data and not an instruction. **Tests:** `TestUntrustedTextMailAndAiHelpers.test_03_prompt_block_and_ai_output_filter`.
