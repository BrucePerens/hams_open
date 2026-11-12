# Data structures

### `FormShieldController` [@ANCHOR: form_shield:form_shield_form_shield_api_form_shield_controller]

**Purpose and location.** The class has no docstring; from its definition it provides `challenge`. Defined in `form_shield/controllers/form_shield_api.py:8` as a class; bases: `http.Controller`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `challenge`.

**See also:** test `test_challenge_endpoint_issues_signed_uncacheable_json (tests/test_http.py)`.

### `FormShield` [@ANCHOR: form_shield:form_shield_form_shield_form_shield]

**Purpose and location.** Model description: Form shield (anti-spam checks for public forms). Defined in `form_shield/models/form_shield.py:58` as an Odoo model; bases: `models.AbstractModel`. (model `form.shield`.) It is an abstract model: it has no table and is only mixed into others.

**Fields.** none declared by this class.

**Class attributes.** `_name = "form.shield"`; `_description = "Form shield (anti-spam checks for public forms)"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_int_param`, `_signing_key`, `_client_address`, `issue`, `_redis`, `_key`, `_reject`, `refuse`, `check`, `check_form`, `_check_response`, `_claim_nonce`, `_over_limit`.

**See also:** test `test_challenge_endpoint_issues_signed_uncacheable_json (tests/test_http.py)`.

### `FormShieldRejection` [@ANCHOR: form_shield:form_shield_form_shield_rejection_form_shield_rejection]

**Purpose and location.** One counter per day, form and reason. No address, no email, no text a visitor typed. Defined in `form_shield/models/form_shield_rejection.py:10` as an Odoo model; bases: `models.Model`. (model `form.shield.rejection`; default order "day desc, form_id, reason".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | no label or help in the source (compute=`"_compute_name"`) |
| `day` | `Date` | no label or help in the source (required=`True`; index=`True`) |
| `form_id` | `Char` | Form (required=`True`; index=`True`) |
| `reason` | `Char` | no label or help in the source (required=`True`; index=`True`) |
| `count` | `Integer` | no label or help in the source (required=`True`; default=`0`) |

**Class attributes.** `_name = "form.shield.rejection"`; `_description = "Form shield daily counters"`; `_order = "day desc, form_id, reason"`; `_day_form_reason_unique = models.Constraint(
        "unique(day, form_id, reason)", "One counter per day, form and reason."
 `.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_compute_name`, `_count`, `_upsert`, `_cron_purge`.

**See also:** test `test_challenge_endpoint_issues_signed_uncacheable_json (tests/test_http.py)`.

### `IrHttp` [@ANCHOR: form_shield:form_shield_ir_http_ir_http]

**Purpose and location.** Extends the Odoo model "ir.http" with no new fields and the methods `_verify_request_recaptcha_token`. Defined in `form_shield/models/ir_http.py:36` as an Odoo model; bases: `models.AbstractModel`. (extends `ir.http`.) It is an abstract model: it has no table and is only mixed into others.

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "ir.http"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_verify_request_recaptcha_token`.

**See also:** test `test_honeypot_marks_a_program (tests/test_form_shield.py)`.

### `Parsed` [@ANCHOR: form_shield:form_shield_tokens_parsed]

**Purpose and location.** The fields of a challenge whose signature has been checked. Defined in `form_shield/tokens.py:108` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`.

**See also:** test `test_tampered_challenge_is_refused (tests/test_tokens.py)`.
