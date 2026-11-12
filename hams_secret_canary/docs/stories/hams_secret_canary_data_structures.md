# Data structures

### `Alarm` [@ANCHOR: hams_secret_canary:hams_secret_canary_alerts_alarm]

**Purpose and location.** The class has no docstring; from its definition it provides class attributes only. Defined in `hams_secret_canary/alerts.py:16` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_pager_backend_opens_one_critical_incident_without_the_value (tests/test_canary.py)`.

### `IrHttp` [@ANCHOR: hams_secret_canary:hams_secret_canary_ir_http_ir_http]

**Purpose and location.** Extends the Odoo model "ir.http" with no new fields and the methods `_post_dispatch`. Defined in `hams_secret_canary/models/ir_http.py:10` as an Odoo model; bases: `models.AbstractModel`. (extends `ir.http`.) It is an abstract model: it has no table and is only mixed into others.

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "ir.http"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_post_dispatch`.

**See also:** test `test_a_response_post_dispatch_already_scanned_is_not_scanned_again (tests/test_canary.py)`.

### `SecretCanary` [@ANCHOR: hams_secret_canary:hams_secret_canary_secret_canary_secret_canary]

**Purpose and location.** Model description: Secret Canary. Defined in `hams_secret_canary/models/secret_canary.py:73` as an Odoo model; bases: `models.AbstractModel`. (model `hams.secret.canary`.) It is an abstract model: it has no table and is only mixed into others.

**Fields.** none declared by this class.

**Class attributes.** `_name = "hams.secret.canary"`; `_description = "Secret Canary"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_register_hook`, `_load_state`, `_store_state`, `_register_secret`, `_plant_canaries`, `_plant_decoy_file_and_env`, `_scan_text`.

**See also:** test `test_registering_a_non_token_fails (tests/test_canary.py)`.

### `SecurityLog` [@ANCHOR: hams_secret_canary:hams_secret_canary_security_log_security_log]

**Purpose and location.** Extends the Odoo model "zero_sudo.security.log" with the fields `reason`. Defined in `hams_secret_canary/models/security_log.py:7` as an Odoo model; bases: `models.Model`. (extends `zero_sudo.security.log`.)

| Field | Type | Meaning |
|---|---|---|
| `reason` | `Selection` | no label or help in the source (ondelete=`{"secret_canary": "cascade"}`) |

**Class attributes.** `_inherit = "zero_sudo.security.log"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_pager_backend_opens_one_critical_incident_without_the_value (tests/test_canary.py)`.
