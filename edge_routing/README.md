# Edge Routing

The `edge_routing` module is the foundational routing layer for high-speed slug caching, vanity URL resolution, and custom domain routing.

## Developer & AI Reference

### Models and Mixins
* **`edge.routing.mixin` (`models/routing_mixin.py`)**:
  * `_generate_unique_slug()`: Generates unique slugs and handles collision avoidance against `RESERVED_SLUGS`.
  * `get_record_by_slug(slug)`: High-performance method wrapped with `@distributed_cache()` for ultra-fast record retrieval by slug.
  * `get_record_by_domain(domain)`: High-performance method wrapped with `@distributed_cache()` for ultra-fast record retrieval by custom domain.
* **`edge.routing.domain` (`models/domain.py`)**:
  * Manages custom domain mapping.
  * `push_all_to_pager_duty()`: Asynchronously synchronizes routing tables and domain configurations with PagerDuty.
* **`res.users` Extension (`models/res_users.py`)**:
  * Provides virtual slug fallback capabilities. When a profile slug is missing, it dynamically resolves the user profile using their unique login (Callsign).

### Utilities
* **`utils.py`**:
  * Contains the global `RESERVED_SLUGS` list to prevent users from claiming system routes.
  * Provides the `slugify(s)` utility function for safe string-to-slug conversion.
