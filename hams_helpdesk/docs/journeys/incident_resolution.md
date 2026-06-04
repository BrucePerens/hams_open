# Journey: Incident Resolution

**[@ANCHOR: journey_incident_resolution]**

This journey describes how an SRE operator resolves a critical system incident using the Hams Helpdesk (**[@ANCHOR: test_helpdesk_operator_tour]**).

1.  **Detection**: An automated alert triggers the creation of a new ticket (`[@ANCHOR: helpdesk_ticket_creation]`).
2.  **Assignment**: The ticket is automatically assigned to the on-duty admin.
3.  **Triage**: The operator receives a toast notification, opens the ticket, and moves it to "In Progress" (`[@ANCHOR: helpdesk_ticket_lifecycle]`).
4.  **Investigation**: The operator adds internal notes and communicates with stakeholders via the chatter.
5.  **Resolution**: The operator implements a fix, updates the ticket status to "Resolved".
6.  **Closure**: After verifying the fix, the ticket is moved to "Closed".

