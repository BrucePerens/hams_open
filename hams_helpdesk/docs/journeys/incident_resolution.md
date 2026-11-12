# Journey: Incident Resolution

## The journey [@ANCHOR: hams_helpdesk:journey_incident_resolution]

This journey describes how an SRE operator resolves a critical system incident using the Hams Helpdesk.

1.  **Detection**: An automated alert triggers the creation of a new ticket (`[@ANCHOR: hams_helpdesk:helpdesk_ticket_creation]`).
2.  **Assignment**: The ticket is automatically assigned to the on-duty admin (as defined in the Architecture section of [the module README](../../README.md); with no one on duty the ticket stays unassigned).
3.  **Triage**: The operator receives a toast notification, opens the ticket, and moves it to "In Progress" (`[@ANCHOR: hams_helpdesk:helpdesk_ticket_lifecycle]`).
4.  **Investigation**: The operator adds internal notes and communicates with stakeholders via the chatter.
5.  **Resolution**: The operator implements a fix, updates the ticket status to "Resolved".
6.  **Closure**: After verifying the fix, the ticket is moved to "Closed".

**Interface:** the journey has no code of its own; it strings together four existing parts. Creation (`hams_helpdesk.ticket.create()`, documented in `helpdesk_ticket_creation.md`) assigns the on-duty admin when `pager_duty` supplies one and subscribes the customer. Stage moves are plain writes of the `stage` field by staff; each one except into Spam mails the customer a "Ticket Update" note. Staff can also close with the Close Ticket header button (`action_close`). Chatter notes posted as internal notes are never shown to the customer. **Errors and limits:** there is no enforced order of stages, no SLA timer and no automatic closure; a ticket stays open until a person moves it. **Tests:** `test_01_ticket_creation_and_routing`, `test_action_close_sets_stage_and_is_usable_by_non_manager_staff` (`tests/test_helpdesk_core.py`), and the operator tour.

