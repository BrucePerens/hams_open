# Data structures

### `ContentViolationReport` [@ANCHOR: content_moderation:content_moderation_content_violation_report_content_violation_report]

**Purpose and location.** Content-type-agnostic report against a reachable piece of content.  Extraction note (2026-09-23): this model used to live entirely inside `user_websites`, keyed by `target_url` and a `content_group_id` hard-typed to `user.websites.group`. It was already generic at its core -- nothing here actually required a personal/group website -- so the extraction moved the model itself here unchanged (same `_name`, so no data migration: the underlying `content_violation_report` table doesn't care which addon's `ir.model.data` row claims to own the model) and left `content_group_id` behind for `user_websites` to add back via its own `_inherit = "content.violation.report"` (models/ content_violation_report_moderation.py), the same way any other module-specific extension field would be added to a shared model. `content_owner_id` (a plain `res.users` reference) stayed here: unlike `content_group_id`, it makes no assumption about violation_strike_count or any other user_websites-only field -- it is genuinely just "who to blame for this content," which every consumer this module was extracted for (forum posts, classifieds, private messages, QSO (two-way radio contact) recordings, and user_websites' own pages) can resolve the same way. Defined in `content_moderation/models/content_violation_report.py:10` as an Odoo model; bases: `models.Model`. (model `content.violation.report`; inherits ["mail.thread", "mail.activity.mixin"]; default order "create_date desc".)

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `target_url` | `Char` | Reported URL (required=`True`; index=`True`) |
| `description` | `Text` | Violation Description (required=`True`) |
| `state` | `Selection` | Status (values `new`, `under_review`, `action_taken`, `dismissed`; default=`"new"`; index=`True`) |
| `content_owner_id` | `Many2one` | Content Owner (comodel `res.users`; ondelete=`"set null"`) |
| `reported_by_user_id` | `Many2one` | Reported By (Internal User) (comodel `res.users`; ondelete=`"set null"`) |
| `reported_by_email` | `Char` | Reported By (Guest Email) |
| `company_id` | `Many2one` | Company (comodel `res.company`; required=`True`; default=`lambda self: self.env.company`) |

**Class attributes.** `_name = "content.violation.report"`; `_description = "Content Violation Report"`; `_inherit = ["mail.thread", "mail.activity.mixin"]`; `_order = "create_date desc"`; `_report_uniq = models.Constraint("UNIQUE(target_url, reported_by_user_id)", "You have already submitted a report fo`; `_url_not_empty = models.Constraint("CHECK(LENGTH(TRIM(target_url)) > 0)", "The target URL cannot be empty.")`; `_desc_not_empty = models.Constraint("CHECK(LENGTH(TRIM(description)) > 0)", "The description cannot be empty.")`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `action_mark_under_review`, `action_dismiss`, `action_take_action_and_strike`, `_apply_enforcement_action`.

**See also:** test `test_01_create_report (tests/test_content_violation_report.py)`.
