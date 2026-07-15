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
