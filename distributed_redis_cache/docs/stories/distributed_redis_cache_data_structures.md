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

### `UpgradeError` (scripts/odoo_upgrade.py)

**Purpose and location.** The code anchor is `distributed_redis_cache_scripts_odoo_upgrade_upgrade_error` (the anchor scan skips the `scripts/` directory, so this entry carries no anchor tag of its own). The exception `odoo_upgrade.py` (the supervised module upgrade tool in `distributed_redis_cache/scripts/`) raises when a precondition of the upgrade fails: an invalid `--db`, `--modules` or `--install` value (including one with a trailing newline), `--shell` combined with `--modules` or `--install`, the Odoo service still running, a service with no `EnvironmentFile=` or whose files do not define the Redis secret, an unreadable environment file, or an unknown service `User=`. Defined in the same file as a plain `Exception` subclass with no body.

**Definition.** `class UpgradeError(Exception)`; the argument is a message.

**Message contents.** The message is safe to print: it never contains a value read from an environment file (only the names of keys, paths and the service name). `main()` catches it, prints `refusing: <message>` to standard error and returns exit status 2; every other exception is a bug and propagates. Nothing is changed on the system before the error is raised: the checks run before the upgrade command is built.

**Concurrency.** Single-threaded command-line tool; the exception is raised and caught in one process and is never stored or sent.

**Functions that touch it.** `require_redis_credentials`, `parse_args` and the argument checks in `run`, which raise it, and `main`, which reports it.

**See also:** the tests in `distributed_redis_cache/scripts/test_odoo_upgrade.py` that call `run` with bad arguments and a fake service description and expect `UpgradeError` with the matching message.
