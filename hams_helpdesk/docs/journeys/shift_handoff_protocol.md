# Journey: Shift Handoff Protocol

## The journey [@ANCHOR: hams_helpdesk:journey_shift_handoff]

Ensuring continuity during shift rotations is critical. This journey outlines the formal handoff process.

1.  **Initiation**: As a shift ends, the outgoing operator reviews active tickets (`[@ANCHOR: hams_helpdesk:helpdesk_shift_handoff]`).
2.  **Context Capture**: For each critical ticket, the operator initiates a "Shift Handoff".
3.  **Wizard Entry**: The operator selects the incoming shift assignee and provides detailed briefing notes.
4.  **Execution**: The system atomically transfers ownership and logs the briefing to the unalterable chatter (`[@ANCHOR: hams_helpdesk:helpdesk_handoff_execution]`).
5.  **Assumption**: The incoming operator sees the ticket in their "My Shift" dashboard and proceeds with full context.

**Interface:** the journey is two calls and one form. `hams_helpdesk.ticket.action_shift_handoff()` (the header button, hidden once the ticket is Resolved or Closed) opens the transient `hams_helpdesk.shift_handoff` form with the ticket and the current assignee as defaults and writes nothing. The wizard has the fields `ticket_id`, `old_user_id` (read only), `new_user_id` and `handoff_notes`; the last two are required, so the form cannot be confirmed without a next assignee and a briefing. `action_confirm_handoff()` then performs the transfer (next entry). **Limits:** the handoff is one ticket at a time, and the "My Shift" menu of step 5 is the action `action_helpdesk_my_shift` ("My Active Shift", `views/dashboard_views.xml`), a list and form of the tickets assigned to the current user (`user_id = uid`) whose stage is New or In Progress, so a handed-off ticket appears there at once for the new assignee. **Tests:** `test_02_shift_handoff_wizard`, the operator tour.

## Execution of the handoff [@ANCHOR: hams_helpdesk:COMM_helpdesk_handoff_execution]

`hams_helpdesk.shift_handoff.action_confirm_handoff()` (`models/shift_handoff.py`, `@public_rpc`, exactly one wizard record or `ensure_one()` raises) does the whole transfer under the `hams_helpdesk.user_helpdesk_service` account, so the write and the note succeed even for an operator with narrow rights. It writes `user_id` on the ticket to the new assignee with `mail_notrack` (no automatic tracking message), then posts one briefing note, subject "Shift Handoff: <ticket name>", that lists who relinquished (or "Unassigned"), who accepted and the notes. All three values are inserted through `markupsafe.Markup.format`, so markup in the notes is escaped (a stored `<script>` reads `&lt;script&gt;`). The note is posted with the subtype `mail.mt_note` (internal), addressed to the new assignee's partner, so the customer's portal thread never shows it. It returns `{"type": "ir.actions.act_window_close"}`. **Errors:** a missing service account raises and nothing is caught; there is no rollback beyond the request's own transaction. **Concurrency:** none beyond the ORM row write; two simultaneous handoffs of the same ticket both succeed and the last write wins.

## Verified by the shift handoff test [@ANCHOR: hams_helpdesk:test_02_shift_handoff_wizard]

`test_02_shift_handoff_wizard` in `tests/test_helpdesk_core.py` creates a ticket assigned to the manager and a second operator in the helpdesk user group, fills the wizard with both users and the notes "Proceed with DB restart...", calls `action_confirm_handoff()` and asserts that `ticket.user_id` is now the new operator and that the ticket's messages contain "Official Shift Handoff Executed" and the notes. A neighbouring test posts notes containing a `<script>` element and asserts that the stored body holds the escaped form and never the raw tag.
