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
    model's real ACL. [@ANCHOR: edge_routing_push_pager_duty_cron_runs]
* **`res.users` Extension (`models/res_users.py`)**:
  * Adds `edge.routing.mixin` to `res.users`, so users resolve by `website_slug` exactly like any other routing model, through the same cache. There is no login fallback: a slug that matches no `website_slug` resolves to nothing, even if it equals a user's login. (An earlier fallback to `login` was removed because logins are email addresses, so it let anyone test whether an account existed for an address.)

### Utilities
* **`utils.py`**:
  * Contains the global `RESERVED_SLUGS` list to prevent users from claiming system routes.
  * Provides the `slugify(s)` utility function for safe string-to-slug conversion.
