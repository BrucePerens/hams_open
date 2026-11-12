# Data structures

### `CloudflareTunnel` [@ANCHOR: tenant_sites:tenant_sites_cloudflare_tunnel_cloudflare_tunnel]

**Purpose and location.** Extends the Odoo model "cloudflare.tunnel" with no new fields and the methods `_ingress_problems`, `_static_rule_problems`. Defined in `tenant_sites/models/cloudflare_tunnel.py:5` as an Odoo model; bases: `models.Model`. (extends `cloudflare.tunnel`.)

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "cloudflare.tunnel"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_ingress_problems`, `_static_rule_problems`.

**See also:** test `test_an_unscoped_path_rule_is_refused_while_tenants_exist (tests/test_models.py)`.

### `IrAsset` [@ANCHOR: tenant_sites:tenant_sites_ir_asset_ir_asset]

**Purpose and location.** Extends the Odoo model "ir.asset" with no new fields and the methods `_get_active_addons_list`, `_get_related_assets`. Defined in `tenant_sites/models/ir_asset.py:5` as an Odoo model; bases: `models.Model`. (extends `ir.asset`.)

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "ir.asset"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_get_active_addons_list`, `_get_related_assets`.

**See also:** test `test_changing_or_removing_a_site_clears_the_asset_and_registry_caches (tests/test_models.py)`.

### `IrHttp` [@ANCHOR: tenant_sites:tenant_sites_ir_http_ir_http]

**Purpose and location.** Extends the Odoo model "ir.http" with no new fields and the methods `_tenant_classify`, `_tenant_compute_kind`, `_tenant_extra_kind`, `_tenant_public_route`, `_match`, `_tenant_match`. Defined in `tenant_sites/models/ir_http.py:35` as an Odoo model; bases: `models.AbstractModel`. (extends `ir.http`.) It is an abstract model: it has no table and is only mixed into others.

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "ir.http"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_tenant_classify`, `_tenant_compute_kind`, `_tenant_extra_kind`, `_tenant_public_route`, `_match`, `_tenant_match`, `_tenant_route_module`, `_tenant_refuse`, `_tenant_ensure_request_uri`, `_serve_fallback`, `_serve_redirect`, `_tenant_serve_other`, `_tenant_plain_response`, `_post_dispatch`.

**See also:** test `test_a_request_without_request_uri_is_a_404_not_a_server_error (tests/test_http.py)`.

### `TenantSiteAccessMixin` [@ANCHOR: tenant_sites:tenant_sites_scoped_content_tenant_site_access_mixin]

**Purpose and location.** Model description: A record named in a URL on a tenant website must belong to that website. Defined in `tenant_sites/models/scoped_content.py:18` as an Odoo model; bases: `models.AbstractModel`. (model `tenant.site.access.mixin`.) It is an abstract model: it has no table and is only mixed into others.

**Fields.** none declared by this class.

**Class attributes.** `_name = "tenant.site.access.mixin"`; `_description = "A record named in a URL on a tenant website must belong to that website"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `can_access_from_current_website`.

**See also:** test `test_the_public_user_cannot_read_any_tenant_table (tests/test_models.py)`.

### `TenantSiteScopedMixin` [@ANCHOR: tenant_sites:tenant_sites_scoped_content_tenant_site_scoped_mixin]

**Purpose and location.** Model description: Search only the tenant website's own records while serving a tenant request. Defined in `tenant_sites/models/scoped_content.py:32` as an Odoo model; bases: `models.AbstractModel`. (model `tenant.site.scoped.mixin`; inherits ["tenant.site.access.mixin"].) It is an abstract model: it has no table and is only mixed into others.

**Fields.** none declared by this class.

**Class attributes.** `_name = "tenant.site.scoped.mixin"`; `_inherit = ["tenant.site.access.mixin"]`; `_description = "Search only the tenant website's own records while serving a tenant request"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_search`.

**See also:** test `test_a_post_named_in_the_url_must_belong_to_the_tenant_website (tests/test_http.py)`.

### `WebsitePage` [@ANCHOR: tenant_sites:tenant_sites_scoped_content_website_page]

**Purpose and location.** Extends the Odoo model ["website.page", "tenant.site.access.mixin"] with no new fields and the methods `_get_page_info`. Defined in `tenant_sites/models/scoped_content.py:47` as an Odoo model; bases: `models.Model`. (model `website.page`; inherits ["website.page", "tenant.site.access.mixin"].)

**Fields.** none declared by this class.

**Class attributes.** `_name = "website.page"`; `_inherit = ["website.page", "tenant.site.access.mixin"]`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_get_page_info`.

**See also:** test `test_a_tenant_site_gets_the_plain_layout_without_other_authors_additions (tests/test_http.py)`.

### `BlogBlog` [@ANCHOR: tenant_sites:tenant_sites_scoped_content_blog_blog]

**Purpose and location.** Extends the Odoo model ["blog.blog", "tenant.site.scoped.mixin"] with no new fields. Defined in `tenant_sites/models/scoped_content.py:71` as an Odoo model; bases: `models.Model`. (model `blog.blog`; inherits ["blog.blog", "tenant.site.scoped.mixin"].)

**Fields.** none declared by this class.

**Class attributes.** `_name = "blog.blog"`; `_inherit = ["blog.blog", "tenant.site.scoped.mixin"]`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_a_post_named_in_the_url_must_belong_to_the_tenant_website (tests/test_http.py)`.

### `BlogPost` [@ANCHOR: tenant_sites:tenant_sites_scoped_content_blog_post]

**Purpose and location.** Extends the Odoo model ["blog.post", "tenant.site.scoped.mixin"] with no new fields. Defined in `tenant_sites/models/scoped_content.py:76` as an Odoo model; bases: `models.Model`. (model `blog.post`; inherits ["blog.post", "tenant.site.scoped.mixin"].)

**Fields.** none declared by this class.

**Class attributes.** `_name = "blog.post"`; `_inherit = ["blog.post", "tenant.site.scoped.mixin"]`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_tenant_blog_shows_only_its_own_posts (tests/test_http.py)`.

### `TenantSite` [@ANCHOR: tenant_sites:tenant_sites_tenant_site_tenant_site]

**Purpose and location.** Model description: Tenant website. Defined in `tenant_sites/models/tenant_site.py:12` as an Odoo model; bases: `models.Model`. (model `tenant.site`; default order "name".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | no label or help in the source (required=`True`) |
| `active` | `Boolean` | no label or help in the source (default=`True`) |
| `website_id` | `Many2one` | The website that answers for this tenant's hostnames. Pages, blogs, menus and redirects of other websites, and records shared by every website, are never shown. (comodel `website`; required=`True`; ondelete=`"restrict"`) |
| `host_ids` | `One2many` | Hostnames (comodel `tenant.site.host`) |
| `static_service` | `Char` | Static file service: Where this site's /static/ is served from when it is not Odoo, for example http://localhost:18201 (the read-only static file server). Every hostname of the site must then have a tunnel route for ^/static/ to this service, and a tunnel push that lacks one is refused (it would silently send the site's files to Odoo, which answers 404). Leave empty when Odoo serves it. |
| `notes` | `Text` | no label or help in the source |

**Class attributes.** `_name = "tenant.site"`; `_description = "Tenant website"`; `_order = "name"`; `_website_uniq = models.Constraint(
        "UNIQUE (website_id)", "This website already belongs to a tenant site."
 `.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_is_tenant_website`, `create`, `write`, `unlink`, `_apply_public_layout`, `_foreign_module_names`, `_is_stock_module`, `_service_env`, `_foreign_layout_views`, `_hide_foreign_layout_views`.

**See also:** test `test_a_tenant_site_gets_the_plain_layout_without_other_authors_additions (tests/test_http.py)`.

### `TenantSiteHost` [@ANCHOR: tenant_sites:tenant_sites_tenant_site_host_tenant_site_host]

**Purpose and location.** Model description: Hostname of a tenant website. Defined in `tenant_sites/models/tenant_site_host.py:11` as an Odoo model; bases: `models.Model`. (model `tenant.site.host`; default order "name".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Hostname (required=`True`; index=`True`) |
| `site_id` | `Many2one` | no label or help in the source (comodel `tenant.site`; required=`True`; index=`True`; ondelete=`"cascade"`) |
| `website_id` | `Many2one` | mirrors the related field `"site_id.website_id"` (related=`"site_id.website_id"`; store=`True`; index=`True`) |

**Class attributes.** `_name = "tenant.site.host"`; `_description = "Hostname of a tenant website"`; `_order = "name"`; `_name_uniq = models.Constraint("UNIQUE (name)", "This hostname already belongs to a tenant site.")`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_normalize_vals`, `create`, `write`, `_check_not_own_host`, `_service_env`, `_own_host_patterns`, `_website_id_for_host`, `_tenant_request_website_id`, `_tenant_hosts_exist`.

**See also:** test `test_a_tenant_site_gets_the_plain_layout_without_other_authors_additions (tests/test_http.py)`.

### `TenantSiteKeptModule` [@ANCHOR: tenant_sites:tenant_sites_tenant_site_kept_module_tenant_site_kept_module]

**Purpose and location.** Model description: Module of another author whose layout and assets tenant sites keep. Defined in `tenant_sites/models/tenant_site_kept_module.py:5` as an Odoo model; bases: `models.Model`. (model `tenant.site.kept.module`.) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Module: Technical name. A tenant site normally shows only Odoo's own layout and assets; a module listed here (for example one that applies a content security policy to every page) is kept. (required=`True`) |

**Class attributes.** `_name = "tenant.site.kept.module"`; `_description = "Module of another author whose layout and assets tenant sites keep"`; `_name_uniq = models.Constraint("UNIQUE (name)", "This module is already listed.")`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `create`, `write`, `unlink`.

**See also:** test `test_a_tenant_site_gets_the_plain_layout_without_other_authors_additions (tests/test_http.py)`.

### `TenantSiteOwnHost` [@ANCHOR: tenant_sites:tenant_sites_tenant_site_own_host_tenant_site_own_host]

**Purpose and location.** Model description: Hostname pattern of the main site. Defined in `tenant_sites/models/tenant_site_own_host.py:8` as an Odoo model; bases: `models.Model`. (model `tenant.site.own.host`; default order "name".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Hostname or pattern: A hostname (example.com) or a leading wildcard (*.example.com). Requests that come through Cloudflare for these names are served exactly as before; every other hostname that is not a tenant site or a parked domain gets a 404. (required=`True`) |

**Class attributes.** `_name = "tenant.site.own.host"`; `_description = "Hostname pattern of the main site"`; `_order = "name"`; `_name_uniq = models.Constraint("UNIQUE (name)", "This pattern is already listed.")`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_normalize_vals`, `create`, `write`.

**See also:** test `test_a_tenant_site_gets_the_plain_layout_without_other_authors_additions (tests/test_http.py)`.

### `Website` [@ANCHOR: tenant_sites:tenant_sites_website_website]

**Purpose and location.** Extends the Odoo model "website" with the fields `tenant_site_ids` and the methods `_get_current_website_id`. Defined in `tenant_sites/models/website.py:5` as an Odoo model; bases: `models.Model`. (extends `website`.)

| Field | Type | Meaning |
|---|---|---|
| `tenant_site_ids` | `One2many` | Tenant sites (comodel `tenant.site`) |

**Class attributes.** `_inherit = "website"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_get_current_website_id`.

**See also:** test `test_tenant_hostnames_select_the_tenant_website (tests/test_models.py)`.
