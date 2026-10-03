# Story: Helpdesk Ticket Lifecycle

**[@ANCHOR: COMM_helpdesk_ticket_lifecycle]**

The helpdesk ticket lifecycle ensures that every reported incident is tracked from detection to closure with a full audit trail.

1.  **Detection**: An incident is reported by a user or automated system. The Helpdesk app is accessible from the main dashboard.
2.  **Assignment**: The ticket is assigned to an operator (often automatically).
3.  **Triage**: The operator reviews the ticket and moves it to "In Progress".
4.  **Resolution**: Once fixed, the ticket is moved to "Resolved".
5.  **Validation**: The reporter validates the fix.
6.  **Closure**: The ticket is moved to "Closed".

*Verified by [@ANCHOR: COMM_test_01_ticket_creation_and_routing]*

Operator interface tracking:
<!-- [@ANCHOR: hams_helpdesk:COMM_helpdesk_operator_tour] -->

Portal interface tracking:
<!-- [@ANCHOR: hams_helpdesk:COMM_helpdesk_portal_tour] -->

### Security and Micro-Privileges
**[@ANCHOR: COMM_helpdesk_micro_privilege]**
Portal users cannot modify restricted fields.

### Shift Handoff
**[@ANCHOR: COMM_helpdesk_shift_handoff]**
Operators can formally hand off tickets to the next shift.

### Portal Close
**[@ANCHOR: COMM_helpdesk_portal_close]**
Portal users can close their own tickets.

### Staff Close
**[@ANCHOR: hams_helpdesk:COMM_helpdesk_action_close]**
A "Close Ticket" button on the backend ticket form lets staff (`group_helpdesk_user` and
`group_helpdesk_manager`) close a ticket directly, without needing delete rights or relying on
the less discoverable stage statusbar. Found live: a real admin reached for deleting a ticket
outright rather than closing it, because there was no button-shaped way to do so, and an
ordinary staff agent (not a manager) has no delete access on this model at all -- the statusbar
click was their only way to close a ticket before this button existed.

### Spam / Phishing Quarantine
**[@ANCHOR: hams_helpdesk:COMM_helpdesk_message_new_spam_filter]**
Mail sent straight to admin@ or support@ becomes a ticket through `message_new()`, not through
pager_duty's Helpdesk Adapter, so the same mail-ingestion spam filter runs there too. A flagged
message still becomes a ticket (never a silent drop), but it starts in the "Spam / Phishing" stage
with an internal note listing the reasons, and a human can move it back to "New" to recover a false
positive. Production tickets #2 and #43 arrived this way before the filter covered this path.

**[@ANCHOR: hams_helpdesk:COMM_helpdesk_no_mailback_on_spam_stage]**
Moving a ticket into "Spam / Phishing" sends no stage-change mail-back: the From address is either
the spammer (a reply confirms a live inbox) or a real customer whose address was spoofed. Moving
it back out of that stage mails the customer as usual.
