# Hams Helpdesk

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).*

Zero-Sudo compliant, lightweight helpdesk management designed for deep SRE integration.

## Technical Architecture & Anchors

This module operates within strict DevSecOps parameters, ensuring all actions are traceable and privileged escalations are explicitly avoided.

* **Ticket Lifecycle (`[@ANCHOR: helpdesk_ticket_lifecycle]`)**: Defines the stages and constraints of an issue, natively integrating with mail threads.
* **Ticket Creation (`[@ANCHOR: helpdesk_ticket_creation]`)**: Intercepts the ORM `create` method to automatically execute pre-shift CC logic and route to the currently on-duty personnel based on calendar availability.
* **Shift Handoff Initiation (`[@ANCHOR: helpdesk_shift_handoff]`)**: UI action triggering the secure transfer wizard, ensuring the leaving operator leaves context.
* **Handoff Execution (`[@ANCHOR: helpdesk_handoff_execution]`)**: The backend transaction that officially modifies ownership and commits the transfer briefing to the unalterable chatter log.
* **Documentation Injection (`[@ANCHOR: helpdesk_doc_injection]`)**: Hooks into the module's `post_init` process to statically push manual content into the central knowledge base.
