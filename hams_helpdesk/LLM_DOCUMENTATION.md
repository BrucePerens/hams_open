# Hams Helpdesk - Technical Documentation

<system_role>
Zero-Sudo compliant, lightweight helpdesk management system designed for SRE (Site Reliability Engineering) workflows. It prioritizes traceability, minimal privilege, and seamless integration with calendar-based on-duty rotations.
</system_role>

<architecture>
The module implements a reactive ticketing system where assignment is driven by on-duty status. It uses a wizard-based handoff mechanism to ensure context is preserved during operator shifts.

- **Models**:
    - `hams_helpdesk.ticket`: Main ticket entity, inherits `mail.thread` for communication.
    - `hams_helpdesk.shift_handoff`: Transient wizard for secure ownership transfer.
- **Security**: Strict IR rules enforce that portal users only see their own tickets, and helpdesk users see tickets assigned to them or unassigned.
</architecture>

<security_design>
- **Zero-Sudo Compliance**: No `.sudo()` calls are allowed. All privilege elevations must use service accounts.
- **Micro-Privilege**: Uses `res.groups.privilege` to define granular access.
- **Portal Isolation**: Portal users are strictly limited via record rules to their own `partner_id`.
</security_design>

<stories_and_journeys>
- [Ticket Lifecycle Management](hams_helpdesk/docs/stories/ticket_lifecycle.md) - Lifecycle of a support request from creation to resolution.
- [Shift Handoff Protocol](hams_helpdesk/docs/stories/shift_handoff.md) - Formal procedure for transferring ticket ownership between SRE shifts.
- [Incident Response Journey](hams_helpdesk/docs/journeys/incident_resolution.md) - End-to-end journey from automated ticket creation to final closure.
</stories_and_journeys>

<anchors>
- `[@ANCHOR: helpdesk_ticket_lifecycle]`: Ticket stage and state management.
- `[@ANCHOR: helpdesk_ticket_creation]`: Automated routing and CC logic.
- `[@ANCHOR: helpdesk_shift_handoff]`: Handoff wizard initiation.
- `[@ANCHOR: helpdesk_handoff_execution]`: Handoff transaction and audit logging.
- `[@ANCHOR: helpdesk_doc_injection]`: Documentation bootstrapping logic.
</anchors>
