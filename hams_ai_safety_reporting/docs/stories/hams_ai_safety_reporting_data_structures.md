# Data structures

### `HelpdeskTicket` [@ANCHOR: hams_ai_safety_reporting:hams_ai_safety_reporting_helpdesk_ticket_helpdesk_ticket]

**Purpose and location.** Extends the Odoo model "hams_helpdesk.ticket" with the fields `ticket_type`. Defined in `hams_ai_safety_reporting/models/helpdesk_ticket.py:7` as an Odoo model; bases: `models.Model`. (extends `hams_helpdesk.ticket`.)

| Field | Type | Meaning |
|---|---|---|
| `ticket_type` | `Selection` | no label or help in the source (ondelete=`{"ai_safety_concern": "set default"}`) |

**Class attributes.** `_inherit = "hams_helpdesk.ticket"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_ai_safety_concern_ticket_type_survives_alongside_other_modules (tests/test_ai_safety_reporting.py)`.
