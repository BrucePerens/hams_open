# Edge Routing

The `edge_routing` module is the foundational routing layer for high-speed slug caching, vanity URL resolution, and custom domain routing.

## Developer & AI Reference

### Models and Mixins
* **`edge.routing.mixin` (`models/routing_mixin.py`)**:
  * `_generate_unique_slug()`: Generates unique slugs and handles collision avoidance against `RESERVED_SLUGS` and against the `website_slug` of every record of every model that inherits this mixin (they share one vanity-URL namespace). On a collision it appends `-1`, `-2`, ... (up to 1000 attempts, then `ValidationError`) and takes a PostgreSQL transaction-scoped advisory lock on the chosen slug so two concurrent transactions cannot both take it. `create()`/`write()` reject an explicitly requested reserved slug with `ValidationError`, but silently suffix a requested slug that is merely taken; a record created with only a `name` gets a slug generated from it.
  * `get_record_by_slug(slug)`: High-performance method wrapped with `@distributed_cache()` (the `distributed_redis_cache` module's Redis-backed cache, shared by all workers) for ultra-fast record retrieval by slug. It lowercases the slug, matches `website_slug` exactly, and returns the record id or `False`; `write()` and `unlink()` invalidate the cached entries for the old and new slugs.
* **`edge.routing.domain` (`models/domain.py`)**:
  * Manages custom domain mapping: each record maps a domain `name` (must contain a dot; stored lowercased) to a `target_slug` (may not be a reserved slug). The table is a registry, not a request router: nothing in hams_open reads `target_slug` when serving a request, and custom-domain traffic is served through Odoo core's `website.domain`. Its consumers are `push_all_to_pager_duty()` below, `user_websites`' `/api/v1/user_websites/domains` list, and the `cloudflare` module, which extends this model to provision a Cloudflare custom hostname.
  * `push_all_to_pager_duty()`: Asynchronously sends the deduplicated list of domain names (every
    `edge.routing.domain` name, plus every `ham.dns.zone` name when `ham_dns` is installed; no slugs
    or routes) to the `pager_duty` module's `/api/v1/pager_duty/update_domains` JSON-RPC route on the
    Odoo server named by the `odoo_host` config option (default `odoo`), port 8069. "PagerDuty" here
    is hams_open's own `pager_duty` monitoring module, not the commercial PagerDuty service; it makes
    the list the target of its Let's Encrypt "certbot" readiness check. The push is skipped with a
    warning when the `pager_duty.domain_api_identity` system parameter (the shared secret that route
    checks) is unset, and a network error or refusal is logged, never raised. Every domain
    create/write/unlink triggers the cron immediately, and it also runs daily. Scheduled via `ir_cron_push_pager_duty`, which runs as a service account
    deliberately kept read-only on `edge.routing.domain` (this method only ever reads rows to push
    externally, never writes them back) -- Odoo core's `_can_execute_action_on_records()` still
    requires write access to the cron's own `model_id` before it will run at all, so `group_ids` on
    the cron authorizes this specific action for this specific group instead of broadening the
    model's real ACL (access control list). (Entries `COMM_domain_push_pagerduty` and `edge_routing_push_pager_duty_cron_runs` below.)
* **`res.users` Extension (`models/res_users.py`)**:
  * Adds `edge.routing.mixin` to `res.users`, so users resolve by `website_slug` exactly like any other routing model, through the same cache. There is no login fallback: a slug that matches no `website_slug` resolves to nothing, even if it equals a user's login. (An earlier fallback to `login` was removed because logins are email addresses, so it let anyone test whether an account existed for an address.)

### Utilities
* **`utils.py`**:
  * Contains the global `RESERVED_SLUGS` list to prevent users from claiming system routes.
  * Provides the `slugify(s)` utility function for safe string-to-slug conversion.
  * `reserved_slug_words(env)`: every word a member or group site slug may not be: the fixed `RESERVED_SLUGS` list, the first path segment of every route registered in this database's routing map, and every first path segment Odoo's language handling owns. The slug validators and the unique-slug generator in `edge.routing.mixin` all ask this one function, so a slug that would shadow a platform route or a language prefix is refused or suffixed the same way everywhere. [@ANCHOR: edge_routing:reserved_slug_words]
  * `routed_first_segments(env)`: the fixed first path segment of every route in the live routing map (lower-cased; dynamic `<converter>` segments and `static` are left out). A static route wins over the `/<slug>` converter route, so a member who claimed one of these words would own a site nobody could reach, and a platform route added next year would silently shadow a slug already claimed; reading the live map means `RESERVED_SLUGS` does not have to be updated by hand for each new route. While modules are still installing it returns an empty set rather than caching a half-built map; the result is kept on the routing-map object, which Odoo rebuilds whenever it invalidates its routing cache, so it is never stale. [@ANCHOR: edge_routing:routed_first_segments]
  * `language_slugs(env)`: every first path segment Odoo's own language handling owns: for each installed language its code (`en_US`), its URL code (`en`, `fr`) and the bare language prefix (`fr` for `fr_FR`), lower-cased. A site slug equal to one of these would shadow Odoo's `/<lang>/...` routing (`/en_US/` is the home page in English). [@ANCHOR: edge_routing:language_slugs]

## Anchor entries for the PagerDuty domain push

### `edge.routing.domain.push_all_to_pager_duty(self)` [@ANCHOR: edge_routing:COMM_domain_push_pagerduty]

`@api.private @api.model` method in `models/domain.py`; no arguments, returns nothing. It gives the `pager_duty` module the
list of domains it should check for Let's Encrypt readiness. Steps: (1) if the `edge_routing_service_account` user exists
(a SQL check on `ir_model_data`, so the method also works while the module is installing), it works as that account;
(2) it reads every `edge.routing.domain` name in batches of 1000 ordered by id; (3) when `ham.dns.zone` is installed it
adds those zone names, read as `ham_dns.user_dns_api_service` inside a savepoint, and any failure there is logged as a
warning and skipped (a soft dependency); (4) it de-duplicates the names; (5) it reads the system parameter
`pager_duty.domain_api_identity` and, if it is not set, logs a warning and returns without sending (a request the endpoint
would refuse also increments its failed-attempt counter for this source address, so retrying blind would lock the sender
out); (6) it POSTs a JSON-RPC body `{"jsonrpc": "2.0", "method": "call", "params": {"domains": [...], "api_identity":
<secret>}}` to `http://<odoo_host>:8069/api/v1/pager_duty/update_domains` (`odoo_host` from the server configuration,
default `odoo`) with a 5 second timeout; (7) it calls `raise_for_status()` and then checks the JSON body, because that
route answers refusals ("Unauthorized", "Too many attempts", "Empty payload") with HTTP 200: an `error` member or a
`result.status` other than `success` becomes a `ValueError`. A final broad `except` (audit-ignore-catch-all) logs any
failure, including network errors, and does not re-raise, so a dead monitoring endpoint never fails the cron. The secret is
never logged. Blocking: it makes one synchronous HTTP request inside the cron transaction. Idempotent: the receiver
replaces its list each time. Only domain names are sent, no slugs or routes. Tests: `tests/test_domain_push.py` (batching,
missing identity, refusal inside a 200 body, soft dependency).

### Cron run through the real scheduler [@ANCHOR: edge_routing_push_pager_duty_cron_runs]

Names the requirement and test for the cron record `ir_cron_push_pager_duty` ("Edge Routing: Sync domains to PagerDuty",
`data/cron.xml`, every day, `model.push_all_to_pager_duty()`, as `edge_routing.edge_routing_service_account`). Odoo's
`ir.actions.server._can_execute_action_on_records()` demands write access to the cron's own `model_id`
(`edge.routing.domain`) before the action runs at all. The service account is deliberately read-only on that model
(`ir.model.access.csv`), since the method only reads rows, so the server action's `group_ids` names the service group
instead: it authorizes this one action without widening the model's ACL. Without it every scheduled run would fail with
"Forbidden server action", while a test calling the method directly would still pass. Domain create, write and unlink
also trigger this cron at once (debounced by the cron's own `_trigger()`). Test:
`tests/test_domain_push.py::test_cron_actually_runs_through_the_real_scheduler_path` runs it through `ir.actions.server.run`.
