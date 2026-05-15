# Story: Shift Handoff Protocol

**Goal**: Ensure seamless continuity of operations when operators rotate shifts.

## Narrative

As an **Outgoing SRE**, I must transfer my active tickets to the incoming shift operator so that they have the full context required to continue remediation.

1.  **Initiation**: The outgoing operator selects "Shift Handoff" on an active ticket (`[@ANCHOR: helpdesk_shift_handoff]`).
2.  **Context Capture**: A wizard appears requiring the selection of the next assignee and detailed handoff notes.
3.  **Execution**: Upon confirmation, the system atomicaly:
    -   Updates the ticket assignee.
    -   Posts a structured briefing to the chatter, preserving it in the immutable history (`[@ANCHOR: helpdesk_handoff_execution]`).
    -   Notifies the incoming operator of their new responsibility.

## Verification

-   `test_02_shift_handoff_wizard` in `hams_helpdesk/tests/test_helpdesk_core.py`
-   `helpdesk_operator_tour.js`
