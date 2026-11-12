# Data structures

### `ConfigError` [@ANCHOR: hams_rpc_guard:hams_rpc_guard_endpoint_gate_config_error]

**Purpose and location.** A gate setting that does not parse. Raised when it is written; logged and failed closed when it is read. Defined in `hams_rpc_guard/endpoint_gate.py:104` as a class; bases: `ValueError`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_a_bad_cidr_is_a_loud_error_never_an_open_gate (tests/test_endpoint_gate.py)`.

### `Exposure` [@ANCHOR: hams_rpc_guard:hams_rpc_guard_exposure_exposure]

**Purpose and location.** Everything the installed addons declared. Sets of `(model, method)` pairs; `clients` maps a login to its set. Defined in `hams_rpc_guard/exposure.py:46` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`, `merge`.

**See also:** test `test_merge_reads_every_section_and_ignores_junk (tests/test_exposure.py)`.

### `IrConfigParameter` [@ANCHOR: hams_rpc_guard:hams_rpc_guard_ir_config_parameter_ir_config_parameter]

**Purpose and location.** Extends the Odoo model "ir.config_parameter" with no new fields and the methods `_check_hams_rpc_guard_parameters`, `_flush_hams_rpc_guard_cache`, `create`, `write`, `unlink`. Defined in `hams_rpc_guard/models/ir_config_parameter.py:23` as an Odoo model; bases: `models.Model`. (extends `ir.config_parameter`.)

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "ir.config_parameter"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_check_hams_rpc_guard_parameters`, `_flush_hams_rpc_guard_cache`, `create`, `write`, `unlink`.

**See also:** test `test_a_bad_value_cannot_be_written (tests/test_endpoint_gate.py)`.

### `IrHttp` [@ANCHOR: hams_rpc_guard:hams_rpc_guard_ir_http_ir_http]

**Purpose and location.** Extends the Odoo model "ir.http" with no new fields and the methods `_authenticate`, `_refuse_rpc_endpoint_when_closed`. Defined in `hams_rpc_guard/models/ir_http.py:17` as an Odoo model; bases: `models.AbstractModel`. (extends `ir.http`.) It is an abstract model: it has no table and is only mixed into others.

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "ir.http"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_authenticate`, `_refuse_rpc_endpoint_when_closed`.

**See also:** test `test_a_daemon_is_limited_to_its_own_entry (tests/test_guard.py)`.

### `Call` [@ANCHOR: hams_rpc_guard:hams_rpc_guard_policy_call]

**Purpose and location.** The facts about one RPC call. Plain attributes so a test can build one without Odoo.  `model` and `method` are names; `user_class` one of the USER_ constants; `login` the caller's login when it is a service account; `defining_models` the `_name` of every class that defines the method (a mixin's name, the concrete model's), `managed` whether the first class that defines it belongs to one of our addons, `orm_standard` whether the name is a standard ORM method, `marks` the `@public_rpc` marks found on it. Defined in `hams_rpc_guard/policy.py:62` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`.

**See also:** test `test_identity_names_the_local_user (tests/test_unix_listener.py)`.

### `ListenerError` [@ANCHOR: hams_rpc_guard:hams_rpc_guard_unix_listener_listener_error]

**Purpose and location.** The configured socket cannot be created safely. Logged at ERROR; the server runs without the listener. Defined in `hams_rpc_guard/unix_listener.py:74` as a class; bases: `RuntimeError`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_config (tests/test_unix_listener.py)`.
