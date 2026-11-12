# Data structures

### `EdgeRoutingDomain` [@ANCHOR: edge_routing:edge_routing_domain_edge_routing_domain]

**Purpose and location.** Model description: Custom Domain Mapping. Defined in `edge_routing/models/domain.py:16` as an Odoo model; bases: `models.Model`. (model `edge.routing.domain`.) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | e.g. www.myclub.org (required=`True`; copy=`False`) |
| `target_slug` | `Char` | The website_slug this domain maps to (required=`True`) |

**Class attributes.** `_name = "edge.routing.domain"`; `_description = "Custom Domain Mapping"`; `_name_uniq = models.Constraint("UNIQUE(name)", "This domain is already mapped!")`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_check_name`, `push_all_to_pager_duty`, `_trigger_pager_duty_sync`, `create`, `write`, `unlink`.

**See also:** test `test_01_domain_crud (tests/test_custom_domains.py)`.

### `ResUsersEdgeRouting` [@ANCHOR: edge_routing:edge_routing_res_users_res_users_edge_routing]

**Purpose and location.** Extends the Odoo model ["res.users", "edge.routing.mixin"] with the fields `name`. Defined in `edge_routing/models/res_users.py:10` as an Odoo model; bases: `models.Model`. (model `res.users`; inherits ["res.users", "edge.routing.mixin"].)

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (required=`True`) |

**Class attributes.** `_name = "res.users"`; `_inherit = ["res.users", "edge.routing.mixin"]`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_get_routing_models_dynamic (tests/test_routing_mixin.py)`.

### `EdgeRoutingMixin` [@ANCHOR: edge_routing:edge_routing_routing_mixin_edge_routing_mixin]

**Purpose and location.** Model description: Edge Routing Mixin. Defined in `edge_routing/models/routing_mixin.py:21` as an Odoo model; bases: `models.AbstractModel`. (model `edge.routing.mixin`.) It is an abstract model: it has no table and is only mixed into others.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `website_slug` | `Char` | Website Slug: The URL-friendly identifier for the site. Alphanumeric and hyphens only. (index=`"trigram"`) |

**Class attributes.** `_name = "edge.routing.mixin"`; `_description = "Edge Routing Mixin"`; `_website_slug_unique = models.Constraint("EXCLUDE (website_slug WITH =) WHERE (website_slug != '')", "The Website Slug must`; `_website_slug_format = models.Constraint("CHECK(website_slug IS NULL OR website_slug = '' OR website_slug ~ '^[a-z0-9\\-]+$`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_check_reserved_slugs`, `_get_routing_models`, `_check_slug_collision`, `_generate_unique_slug`, `get_record_by_slug`, `create`, `write`, `unlink`.

**See also:** test `test_01_domain_crud (tests/test_custom_domains.py)`.
