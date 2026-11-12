# Data structures

### `ComplianceController` [@ANCHOR: compliance:compliance_main_compliance_controller]

**Purpose and location.** The class has no docstring; from its definition it provides `compliance_index`, `compliance_dmca`. Defined in `compliance/controllers/main.py:6` as a class; bases: `http.Controller`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `compliance_index`, `compliance_dmca`.

**See also:** test `test_compliance_index_route_lists_only_active_documents (tests/test_pages.py)`.

### `Website` [@ANCHOR: compliance:compliance_compliance_config_website]

**Purpose and location.** Extends the Odoo model "website" with the fields `cookies_bar`. Defined in `compliance/models/compliance_config.py:7` as an Odoo model; bases: `models.Model`. (extends `website`.)

| Field | Type | Meaning |
|---|---|---|
| `cookies_bar` | `Boolean` | no label or help in the source (default=`True`) |

**Class attributes.** `_inherit = "website"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_02_post_init_hook_cookie_bar (tests/test_hooks.py)`.

### `ComplianceDocument` [@ANCHOR: compliance:compliance_compliance_document_compliance_document]

**Purpose and location.** Model description: Compliance Document. Defined in `compliance/models/compliance_document.py:7` as an Odoo model; bases: `models.Model`. (model `compliance.document`.) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (required=`True`) |
| `description` | `Text` | Description |
| `url` | `Char` | URL |
| `sequence` | `Integer` | Sequence (default=`10`) |
| `active` | `Boolean` | Active (default=`True`) |

**Class attributes.** `_name = "compliance.document"`; `_description = "Compliance Document"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_execute_gdpr_erasure_deactivates_the_account (tests/test_gdpr_base.py)`.

### `ResUsers` [@ANCHOR: compliance:compliance_res_users_res_users]

**Purpose and location.** Extends the Odoo model "res.users" with no new fields and the methods `_execute_gdpr_erasure`, `_get_gdpr_export_data`, `_get_gdpr_streamed_keys`. Defined in `compliance/models/res_users.py:8` as an Odoo model; bases: `models.Model`. (extends `res.users`.)

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "res.users"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_execute_gdpr_erasure`, `_get_gdpr_export_data`, `_get_gdpr_streamed_keys`.

**See also:** test `test_execute_gdpr_erasure_deactivates_the_account (tests/test_gdpr_base.py)`.
