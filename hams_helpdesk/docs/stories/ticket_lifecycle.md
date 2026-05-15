# Story: Ticket Lifecycle Management

**Goal**: Efficiently track and resolve system issues while maintaining a clear audit trail.

## Narrative

As an **SRE Operator**, I need to track the status of reported issues so that I can prioritize my work and ensure nothing falls through the cracks.

1.  **Incoming Request**: A new ticket is created, either manually or via automated incident detection.
2.  **Automated Routing**: The system identifies the currently on-duty administrator (`[@ANCHOR: helpdesk_ticket_creation]`) and assigns the ticket.
3.  **Progression**: The operator moves the ticket through stages: New -> In Progress -> Resolved -> Closed (`[@ANCHOR: helpdesk_ticket_lifecycle]`).
4.  **Customer Communication**: Every stage change triggers an automated update to the reporter, ensuring they are kept in the loop without manual overhead.

## Verification

-   `test_01_ticket_creation_and_routing` in `hams_helpdesk/tests/test_helpdesk_core.py`
-   `test_04_stage_mailback_automation` in `hams_helpdesk/tests/test_helpdesk_core.py`
