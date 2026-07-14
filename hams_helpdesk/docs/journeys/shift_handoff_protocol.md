# Journey: Shift Handoff Protocol

**[@ANCHOR: hams_helpdesk:journey_shift_handoff]**

Ensuring continuity during shift rotations is critical. This journey outlines the formal handoff process.

1.  **Initiation**: As a shift ends, the outgoing operator reviews active tickets (`[@ANCHOR: hams_helpdesk:helpdesk_shift_handoff]`).
2.  **Context Capture**: For each critical ticket, the operator initiates a "Shift Handoff".
3.  **Wizard Entry**: The operator selects the incoming shift assignee and provides detailed briefing notes.
4.  **Execution**: The system atomically transfers ownership and logs the briefing to the unalterable chatter (`[@ANCHOR: hams_helpdesk:helpdesk_handoff_execution]`).
5.  **Assumption**: The incoming operator sees the ticket in their "My Shift" dashboard and proceeds with full context.

*Verified by [@ANCHOR: hams_helpdesk:test_02_shift_handoff_wizard]*
<!-- [@ANCHOR: hams_helpdesk:COMM_helpdesk_handoff_execution] -->
