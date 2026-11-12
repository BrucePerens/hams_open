# Data structures

### `CloudflareCsrfToken` [@ANCHOR: edge_cache:edge_cache_csrf_token_cloudflare_csrf_token]

**Purpose and location.** The class has no docstring; from its definition it provides `csrf_token`. Defined in `edge_cache/controllers/csrf_token.py:16` as a class; bases: `http.Controller`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `csrf_token`.

**See also:** test `test_login_page_and_post_targets_are_never_cacheable (tests/test_edge_cache_cookies.py)`.

### `IrHttp` [@ANCHOR: edge_cache:edge_cache_ir_http_ir_http]

**Purpose and location.** Extends the Odoo model "ir.http" with no new fields and the methods `_post_dispatch`, `_handle_error`, `_cloudflare_error_never_stored`, `_cloudflare_edge_cacheable`, `_cloudflare_strip_redundant_cookies`. Defined in `edge_cache/models/ir_http.py:43` as an Odoo model; bases: `models.AbstractModel`. (extends `ir.http`.) It is an abstract model: it has no table and is only mixed into others.

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "ir.http"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_post_dispatch`, `_handle_error`, `_cloudflare_error_never_stored`, `_cloudflare_edge_cacheable`, `_cloudflare_strip_redundant_cookies`.

**See also:** test `test_01_static_asset_caching (tests/test_edge_cache_headers.py)`.

### `WebsitePage` [@ANCHOR: edge_cache:edge_cache_website_page_website_page]

**Purpose and location.** Extends the Odoo model "website.page" with no new fields and the methods `_get_response`, `_get_response_raw`, `_post_process_response_from_cache`. Defined in `edge_cache/models/website_page.py:8` as an Odoo model; bases: `models.Model`. (extends `website.page`.)

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "website.page"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_get_response`, `_get_response_raw`, `_post_process_response_from_cache`.

**See also:** test `test_login_page_and_post_targets_are_never_cacheable (tests/test_edge_cache_cookies.py)`.
