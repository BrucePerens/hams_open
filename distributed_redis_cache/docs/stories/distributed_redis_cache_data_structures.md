# Data structures

### `DistributedCacheConfig` [@ANCHOR: distributed_redis_cache:distributed_redis_cache_distributed_cache_config_distributed_cache_config]

**Purpose and location.** Model description: Distributed Cache Configuration. Defined in `distributed_redis_cache/models/distributed_cache_config.py:20` as an Odoo model; bases: `models.TransientModel`. (model `distributed.cache.config`.) It is a transient (wizard) model: its rows are short-lived and removed by Odoo's vacuum.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `model_id` | `Many2one` | Model to Invalidate: Select a model to flush its specific cache. (comodel `ir.model`) |

**Class attributes.** `_name = "distributed.cache.config"`; `_description = "Distributed Cache Configuration"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `action_invalidate_model_cache`, `check_redis_status`.

**See also:** test `test_distributed_cache_admin_tour (tests/test_distributed_cache.py)`.

### `IrCron` [@ANCHOR: distributed_redis_cache:distributed_redis_cache_ir_cron_ir_cron]

**Purpose and location.** Extends the Odoo model "ir.cron" with no new fields and the methods `_process_job`. Defined in `distributed_redis_cache/models/ir_cron.py:11` as an Odoo model; bases: `models.Model`. (extends `ir.cron`.)

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "ir.cron"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_process_job`.

**See also:** test `test_cron_dispatch_polls_before_running_the_job (tests/test_cron_cache_interceptor.py)`.

### `IrHttp` [@ANCHOR: distributed_redis_cache:distributed_redis_cache_ir_http_ir_http]

**Purpose and location.** Extends the Odoo model "ir.http" with no new fields and the methods `_authenticate`. Defined in `distributed_redis_cache/models/ir_http.py:15` as an Odoo model; bases: `models.AbstractModel`. (extends `ir.http`.) It is an abstract model: it has no table and is only mixed into others.

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "ir.http"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_authenticate`.

**See also:** test `test_b2_3_banned_local_imports (tests/test_b2_fixes.py)`.

### `ResConfigSettings` [@ANCHOR: distributed_redis_cache:distributed_redis_cache_res_config_settings_res_config_settings]

**Purpose and location.** Extends the Odoo model "res.config.settings" with the fields `redis_host`, `redis_port`, `redis_password` and the methods `get_values`, `set_values`. Defined in `distributed_redis_cache/models/res_config_settings.py:15` as an Odoo model; bases: `models.TransientModel`. (extends `res.config.settings`.) It is a transient (wizard) model: its rows are short-lived and removed by Odoo's vacuum.

| Field | Type | Meaning |
|---|---|---|
| `redis_host` | `Char` | Redis Host (default=`"redis"`) |
| `redis_port` | `Integer` | Redis Port (default=`6379`) |
| `redis_password` | `Char` | Redis Password |

**Class attributes.** `_inherit = "res.config.settings"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `get_values`, `set_values`.

**See also:** test `test_redis_pools_authenticate_with_the_redis_env_credentials (tests/test_fixes.py)`.
