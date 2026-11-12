# Data structures

### `RabbitMQPool` [@ANCHOR: hams_rabbitmq:hams_rabbitmq_rabbitmq_pool_rabbit_mqpool]

**Purpose and location.** Model description: Global RabbitMQ Connection Pool. Defined in `hams_rabbitmq/models/rabbitmq_pool.py:105` as an Odoo model; bases: `models.AbstractModel`. (model `hams_rabbitmq.pool`.) It is an abstract model: it has no table and is only mixed into others.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (required=`True`) |

**Class attributes.** `_name = "hams_rabbitmq.pool"`; `_description = "Global RabbitMQ Connection Pool"`; `_connection = None`; `_channel = None`; `_connection_pid = None`; `_last_used = None`; `_lock = _POOL_LOCK`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_get_channel`, `_start_keepalive_thread`, `_keepalive_loop`, `_keepalive_tick`, `_forget_keepalive`, `_shutdown_all`, `_shutdown`, `_retire_unusable_connection`, `_discard_connection`, `publish`.

**See also:** test `test_01_get_channel_connects_with_real_config (tests/test_rabbitmq_pool.py)`.
