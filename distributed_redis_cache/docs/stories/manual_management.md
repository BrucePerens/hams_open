<!-- Copyright © Bruce Perens K6BP. AGPL-3.0-or-later. -->

# Story: Manual Cache Management

While the cache is designed to be fully automated, administrators occasionally need manual control over the invalidation process.

## The Management Interface
The module provides a dedicated UI (`COMM_distributed_cache_view`) where administrators can:
- **Monitor Status**: Verify that Odoo is successfully communicating with the Redis backend.
- **Targeted Invalidation**: Select a specific Odoo model and trigger a manual cache flush (`COMM_manual_cache_invalidation`).

## Redis Configuration
The Redis connection settings can be configured via the **Settings** menu under the **Distributed Redis Cache** section (`COMM_distributed_cache_settings_view`).

## Safety First
Manual invalidation still follows the standard invalidation pipeline, ensuring that the cache is cleared across the *entire* cluster, not just on the administrator's current worker.

## Automatic Documentation
Upon installation, the module automatically injects its comprehensive documentation into the Odoo Knowledge app `COMM_doc_inject_distributed_redis_cache`, ensuring that administrators have immediate access to these instructions.

## Anchor reference

Full entries for the anchors named above.

### `COMM_distributed_cache_view` [@ANCHOR: COMM_distributed_cache_view]

**Purpose.** The administrator's control panel for the cache: a form in which a model can be picked for invalidation and the health of this server's Redis connection checked.

**Definition.** View `view_distributed_cache_config_form` on the transient model `distributed.cache.config` (`views/distributed_cache_views.xml`), opened in a dialog by the action `action_distributed_cache_config`. Fields: `model_id` (Many2one `ir.model`, required, no create) and `name`. Footer buttons: **Invalidate Model Cache** (`action_invalidate_model_cache`, `COMM_manual_cache_invalidation`), **Check Redis Status** (`check_redis_status`) and Cancel. The menu **Settings > Technical > Distributed Cache** (`menu_distributed_cache_root`) is shown to the group `distributed_redis_cache.group_distributed_cache_manager`.

**Behaviour.** Check Redis Status does a `PING` through `get_redis_connection(env)` and shows a green "healthy", a red failure message with the Redis error, or a yellow notice that the local fallback cache is active when Redis is not available. It tests only this Odoo server's connection, not the daemon. The records are transient and hold no cache data; the Redis connection fields live on the Settings page, not here.

**See also.** Tests: `tests/test_distributed_cache.py` and the tour `static/src/js/distributed_cache_tour.js`.

### `COMM_manual_cache_invalidation` [@ANCHOR: COMM_manual_cache_invalidation]

**Purpose.** The **Invalidate Model Cache** button of the Distributed Cache Manager form: lets an administrator flush one model's cached results on demand.

**Call interface.** `distributed.cache.config.action_invalidate_model_cache()` (a `@public_rpc` button method on a transient model; `public_rpc` is the `zero_sudo` marker that lists it as callable). It needs exactly one record (`ensure_one`); the record's `model_id` (Many2one to `ir.model`, required in the form) names the model. Returns an `ir.actions.client` `display_notification` dict (title "Success", message "Cache invalidated for model <model>") when a model is selected, otherwise `None` and nothing happens.

**Special concerns.** It does not clear anything itself: it calls `notify_model_invalidation(env, model)` (see `COMM_notify_model_invalidation_logic`), so the flush follows the normal pipeline and reaches the whole cluster after the transaction commits, not only the administrator's worker. The model name comes from the selected `ir.model` record, so it is always a real model. Access is limited by the form's menu group `distributed_redis_cache.group_distributed_cache_manager` and the model's access rules.

**See also.** Test `tests/test_distributed_cache.py`; view `COMM_distributed_cache_view`.

### `COMM_distributed_cache_settings_view` [@ANCHOR: COMM_distributed_cache_settings_view]

**Purpose.** Where an administrator enters the Redis host, port and password that Odoo workers use.

**Definition.** View `res_config_settings_view_form` (`views/res_config_settings_views.xml`), priority 90, inheriting `base.res_config_settings_view_form`; an xpath adds an `app` block named `distributed_redis_cache` (**Distributed Redis Cache**) with a block **Redis Configuration**. It shows `redis_host` (Char, default `redis`), `redis_port` (Integer, default 6379) and `redis_password` (Char, rendered as a password field), each stored as the system parameters `distributed_redis_cache.redis_host`, `.redis_port` and `.redis_password`.

**Behaviour.** `get_values` replaces a stored password with a mask so the form never reveals it, and `set_values` keeps the stored password when the mask comes back unchanged, then clears the saving worker's cached connection settings and sends the invalidation notification so other workers reconnect at their next request. These settings are not passed to the cache-manager daemon, which reads its own environment.

**See also.** Test `tests/test_distributed_cache.py`; `distributed_redis_cache:res_config_settings_set_values`, `distributed_redis_cache:res_config_settings_get_values_masks_password`.

### `COMM_doc_inject_distributed_redis_cache` [@ANCHOR: COMM_doc_inject_distributed_redis_cache]

**Purpose.** Puts the module's user documentation into the Odoo Knowledge app when the module is installed, so administrators find the instructions inside Odoo.

**Definition.** The page `data/documentation.html` ("Distributed Redis Cache", icon, category `workspace`) is declared in the manifest key `knowledge_docs` with its name, path, icon and category. The injection is done by the shared documentation hook of `zero_sudo` (see `zero_sudo` docs); this module has no code of its own for it. The page explains what the module is for, the Distributed Cache Manager form (Check Redis Status, Invalidate Model Cache), and the Settings page, in plain language for administrators.

**Special concerns.** `zero_sudo` creates or updates the article each time the server loads, when the content changed, and it is visible to administrators unless a module marks it public. If the Knowledge app is not installed nothing is injected and nothing fails. The document carries trace markers for the view and settings anchors. Change the HTML and the article follows on the next load.

**See also.** Tests: the documentation checks in `zero_sudo`; story `docs/stories/manual_management.md`.
