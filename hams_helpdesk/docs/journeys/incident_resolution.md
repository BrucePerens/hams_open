# Journey: Incident Resolution

**Goal**: Complete the lifecycle of a critical incident from detection to resolution.

## Path

1.  **Detection**: System monitor detects a service failure.
2.  **Creation**: A Helpdesk ticket is created (via API or manual entry).
3.  **Assignment**: The system auto-assigns the ticket to the SRE currently "On-Duty" per the PagerDuty calendar.
4.  **Acknowledgment**: The SRE receives a browser notification and opens the ticket.
5.  **Investigation**: SRE updates stage to "In Progress".
6.  **Resolution**: SRE fixes the issue, updates stage to "Resolved", and adds notes.
7.  **Closure**: After verification, the ticket is moved to "Closed".

## Verification

-   `test_01_ticket_creation_and_routing`
-   `helpdesk_operator_tour.js`
