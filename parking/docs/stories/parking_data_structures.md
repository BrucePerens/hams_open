# Data structures

### `ParkingController` [@ANCHOR: parking:parking_parking_api_parking_controller]

**Purpose and location.** The class has no docstring; from its definition it provides `inquiry`, `_rate_limited`, `_done`. Defined in `parking/controllers/parking_api.py:18` as a class; bases: `http.Controller`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `inquiry`, `_rate_limited`, `_done`.

**See also:** test `test_sale_form_end_to_end (tests/test_http.py)`.

### `IrHttp` [@ANCHOR: parking:parking_ir_http_ir_http]

**Purpose and location.** Extends the Odoo model "ir.http" with no new fields and the methods `_tenant_extra_kind`, `_tenant_public_route`, `_tenant_serve_other`, `_parking_service_env`, `_parking_serve`, `_parking_page`. Defined in `parking/models/ir_http.py:25` as an Odoo model; bases: `models.AbstractModel`. (extends `ir.http`.) It is an abstract model: it has no table and is only mixed into others.

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "ir.http"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_tenant_extra_kind`, `_tenant_public_route`, `_tenant_serve_other`, `_parking_service_env`, `_parking_serve`, `_parking_page`, `_parking_response`, `_post_dispatch`, `_parking_secret`.

**See also:** test `test_only_parked_hostnames_are_classified_as_parking (tests/test_http.py)`.

### `ParkingDomain` [@ANCHOR: parking:parking_parking_domain_parking_domain]

**Purpose and location.** Model description: Parked domain. Defined in `parking/models/parking_domain.py:10` as an Odoo model; bases: `models.Model`. (model `parking.domain`; default order "name".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Domain: Lowercase hostname, for example example.com or xn--bcher-kva.example. Internationalized names are stored in punycode. (required=`True`; index=`True`) |
| `active` | `Boolean` | no label or help in the source (default=`True`) |
| `behavior` | `Selection` | no label or help in the source (values `parked`, `redirect`, `for_sale`, `gone`; required=`True`; default=`"parked"`) |
| `redirect_url` | `Char` | Redirect target: Absolute http(s) URL. |
| `redirect_code` | `Selection` | no label or help in the source (values `301`, `302`; default=`"301"`) |
| `preserve_path` | `Boolean` | Keep the path and query: Append the request path and query to the redirect target. The host always comes from the target, never from the request. |
| `include_www` | `Boolean` | Also answer for www.: www.<domain> is served with the same settings when it has no record of its own. (default=`True`) |
| `title` | `Char` | no label or help in the source |
| `message` | `Text` | no label or help in the source |
| `price_text` | `Char` | Price or offer text |
| `noindex` | `Boolean` | Ask search engines not to index: robots.txt and X-Robots-Tag. (default=`True`) |
| `cache_ttl` | `Integer` | Edge cache seconds: Cache-Control s-maxage for this domain's responses. (default=`3600`) |
| `notes` | `Text` | no label or help in the source |
| `inquiry_ids` | `One2many` | no label or help in the source (comodel `parking.inquiry`) |
| `inquiry_count` | `Integer` | no label or help in the source (compute=`"_compute_inquiry_count"`) |

**Class attributes.** `_name = "parking.domain"`; `_description = "Parked domain"`; `_order = "name"`; `_name_uniq = models.Constraint("UNIQUE (name)", "This domain is already configured.")`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_compute_inquiry_count`, `create`, `write`, `_normalize_name`, `_check_behavior`, `_check_not_main_or_tenant`, `_lookup`.

**See also:** test `test_parked_page (tests/test_http.py)`.

### `ParkingInquiry` [@ANCHOR: parking:parking_parking_inquiry_parking_inquiry]

**Purpose and location.** Model description: Inquiry about a for-sale domain. Defined in `parking/models/parking_inquiry.py:5` as an Odoo model; bases: `models.Model`. (model `parking.inquiry`; default order "create_date desc".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `domain_id` | `Many2one` | no label or help in the source (comodel `parking.domain`; required=`True`; index=`True`; ondelete=`"cascade"`) |
| `name` | `Char` | no label or help in the source |
| `email` | `Char` | no label or help in the source (required=`True`) |
| `message` | `Text` | no label or help in the source (required=`True`) |
| `ip_hash` | `Char` | Keyed hash of the visitor address, for rate limiting only. (index=`True`) |

**Class attributes.** `_name = "parking.inquiry"`; `_description = "Inquiry about a for-sale domain"`; `_order = "create_date desc"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_sale_form_end_to_end (tests/test_http.py)`.

### `TenantSiteHost` [@ANCHOR: parking:parking_tenant_site_host_tenant_site_host]

**Purpose and location.** Extends the Odoo model "tenant.site.host" with no new fields and the methods `_tenant_hosts_exist`. Defined in `parking/models/tenant_site_host.py:7` as an Odoo model; bases: `models.Model`. (extends `tenant.site.host`.)

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "tenant.site.host"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_tenant_hosts_exist`.

**See also:** test `test_parked_domains_count_as_tenant_hostnames (tests/test_models.py)`.
