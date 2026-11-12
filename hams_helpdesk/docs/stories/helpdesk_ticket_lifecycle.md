# Story: Helpdesk Ticket Lifecycle

## Lifecycle of a ticket [@ANCHOR: COMM_helpdesk_ticket_lifecycle]

The helpdesk ticket lifecycle ensures that every reported incident is tracked from detection to closure with a full audit trail.

1.  **Detection**: An incident is reported by a user or automated system. The Helpdesk app is accessible from the main dashboard.
2.  **Assignment**: The ticket is assigned to an operator (often automatically).
3.  **Triage**: The operator reviews the ticket and moves it to "In Progress".
4.  **Resolution**: Once fixed, the ticket is moved to "Resolved".
5.  **Validation**: The reporter validates the fix.
6.  **Closure**: The ticket is moved to "Closed".

**Data:** the model is `hams_helpdesk.ticket` (`models/helpdesk_ticket.py`). The `stage` field is a required Selection, default `new`: New, In Progress, Resolved, Closed and, last, Spam / Phishing (Quarantined), a side lane for flagged inbound mail. The code enforces no order; staff may write any stage, a portal user's write of `stage` raises `AccessError`. **Side effects:** each stage change except one into `spam` posts a customer-visible "Ticket Update" message to the ticket's customer (`write()`), which is the mail-back. **Errors:** none of its own; a missing `hams_helpdesk.user_helpdesk_service` account makes creation raise. **See also:** `test_01_ticket_creation_and_routing`, `test_helpdesk_core.py`.

## Verified by the creation and routing test [@ANCHOR: COMM_test_01_ticket_creation_and_routing]

`test_01_ticket_creation_and_routing` in `tests/test_helpdesk_core.py` patches the notification bus and, when `pager_duty` is installed, the on-duty lookup, then creates a ticket for a portal member. It asserts the ticket is assigned to the on-duty manager (left unassigned without `pager_duty`) and that the member's partner is subscribed to the ticket thread, so that stage changes are mailed back. It is the cited test of both this story and the creation story.

## Operator interface tracking [@ANCHOR: hams_helpdesk:COMM_helpdesk_operator_tour]

The tour `helpdesk_operator_tour` (`static/src/tours/helpdesk_operator_tour.js`, run by `test_helpdesk_operator_tour` as the manager user `hd_manager_tour`) opens the ticket list, creates a ticket, saves it, opens the Shift Handoff wizard, picks the next assignee, writes notes, confirms, and waits for the modal to close and the form to reload. It is the browser-level check of the backend list, form and handoff views.

## Portal interface tracking [@ANCHOR: hams_helpdesk:COMM_helpdesk_portal_tour]

The tour `helpdesk_portal_tour` (`static/src/tours/helpdesk_portal_tour.js`, run by `test_helpdesk_portal_tour` as the portal user `portal_cust_tour`) starts at `/my/tickets`, submits a new ticket with subject, callsign and description, checks the detail page (breadcrumb, callsign, status badge), closes the ticket and checks the badge. It is the browser-level check of the portal list, new, detail and close pages.

## Security and Micro-Privileges [@ANCHOR: COMM_helpdesk_micro_privilege]

Portal users cannot modify restricted fields. `write()` checks `base.group_portal` and refuses (`AccessError`) a change by a portal user to stage, the suspicion and triage fields, notification state, assignee, priority, calendar event, website, company, customer (`partner_id`, so a ticket cannot be moved into a stranger's portal), ticket type, the archive flag and every `ncmec_*` child-safety report field. Description and subject remain writable by the owner. Tested by `test_05_portal_write_restrictions` and `test_05b_portal_cannot_reassign_partner_id` in `tests/test_helpdesk_core.py`.

## Shift Handoff [@ANCHOR: COMM_helpdesk_shift_handoff]

Operators can formally hand off tickets to the next shift. `action_shift_handoff()` (`@public_rpc`, one ticket, `ensure_one()` raises otherwise) returns an `ir.actions.act_window` dict that opens the `hams_helpdesk.shift_handoff` wizard in a modal with the defaults `default_ticket_id` and `default_old_user_id` (the current assignee or False). It writes nothing and elevates nothing; the write happens when the wizard is confirmed (`action_confirm_handoff`, see the journey `shift_handoff_protocol.md`). Tested by `test_02_shift_handoff_wizard`.

## Portal Close [@ANCHOR: COMM_helpdesk_portal_close]

Portal users can close their own tickets. `action_portal_close()` (`@api.private`, one ticket) raises `AccessError` unless the caller's partner is the ticket's customer or the caller is in `group_helpdesk_user`. A ticket not yet closed is written to `closed` under the helpdesk service account with `mail_notrack`, and a "Ticket closed by customer." note is posted; a closed ticket is left alone. The route is `POST /my/ticket/<id>/close` (CSRF protected, `auth="user"`). Tested by `test_portal_close_ticket` in `tests/test_ui_tours.py`.

### Staff Close [@ANCHOR: hams_helpdesk:COMM_helpdesk_action_close]
A "Close Ticket" button on the backend ticket form lets staff (`group_helpdesk_user` and
`group_helpdesk_manager`) close a ticket directly, without needing delete rights or relying on
the less discoverable stage statusbar. Found live: a real admin reached for deleting a ticket
outright rather than closing it, because there was no button-shaped way to do so, and an
ordinary staff agent (not a manager) has no delete access on this model at all -- the statusbar
click was their only way to close a ticket before this button existed.

### Spam / Phishing Quarantine [@ANCHOR: hams_helpdesk:COMM_helpdesk_message_new_spam_filter]
Mail sent straight to admin@ or support@ becomes a ticket through `message_new()`, not through
pager_duty's Helpdesk Adapter, so the same mail-ingestion spam filter runs there too. A flagged
message still becomes a ticket (never a silent drop), but it starts in the "Spam / Phishing" stage
with an internal note listing the reasons, and a human can move it back to "New" to recover a false
positive. Production tickets #2 and #43 arrived this way before the filter covered this path.

The call is `inbound_spam_reasons(subject, body_html, signals, verdicts, trusted_sender)`, with the raw message's header facts and HTML
shape (parked by `message_parse`) and the SES verdicts, so bulk marketing mail is caught by its headers and shape, not only by its
subject; mail from a member (a sender with a user account) is never flagged. The full interface, the rules and their weights are in
`helpdesk_ticket_creation.md` ([@ANCHOR: hams_helpdesk:inbound_spam_reasons]). Odoo's own assignment email is switched off for a ticket
created in this stage (`hams_helpdesk:COMM_helpdesk_spam_create_sends_no_mail`).

**[@ANCHOR: hams_helpdesk:COMM_helpdesk_no_mailback_on_spam_stage]**
Moving a ticket into "Spam / Phishing" sends no stage-change mail-back: the From address is either
the spammer (a reply confirms a live inbox) or a real customer whose address was spoofed. Moving
it back out of that stage mails the customer as usual.

**[@ANCHOR: hams_helpdesk:COMM_helpdesk_no_assignment_notice_for_spam]**
A ticket created already in "Spam / Phishing" is still assigned to the on-duty admin, but the
assignment email, the toast and the "Shift CC" note to the next shift's admins are skipped, and Odoo's own auto-subscribe
assignment email is off: quarantined mail pages nobody. A "New" ticket notifies its assignee as before.

### A member can submit a ticket that is routed to an agent [@ANCHOR: hams_helpdesk:portal_submit_service_default_env]
The portal submit form creates the ticket under the helpdesk service account, also for the transaction's access check on the agent assignment note, so a plain member is never refused over contact access.

### A member sees their own tickets under /my/tickets
`portal_my_tickets(page=1)` **[@ANCHOR: hams_helpdesk:portal_my_tickets]** lists the signed-in member's tickets, 20 to a page, newest first, with a pager. It reads under the helpdesk service account and the website's company, filtered to the member's own partner and to tickets of this website or of no website, so a ticket from another site never shows. Login required (`auth="user"`). Tests: `test_member_manual.py` (`test_02_list_page_has_the_names_the_manual_uses`).

