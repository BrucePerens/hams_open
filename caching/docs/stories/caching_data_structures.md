# Data structures

### `ServiceWorkerController` [@ANCHOR: caching:caching_main_service_worker_controller]

**Purpose and location.** The class has no docstring; from its definition it provides `service_worker`. Defined in `caching/controllers/main.py:12` as a class; bases: `http.Controller`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `service_worker`.

**See also:** test `test_01_sw_headers (tests/test_service_worker.py)`.

### `PWAController` [@ANCHOR: caching:caching_pwa_pwacontroller]

**Purpose and location.** The class has no docstring; from its definition it provides `manifest`, `offline`. Defined in `caching/controllers/pwa.py:8` as a class; bases: `http.Controller`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `manifest`, `offline`.

**See also:** test `test_02_offline_fallback_page_renders (tests/test_service_worker.py)`.

### `CachingMixin` [@ANCHOR: caching:caching_caching_mixin_caching_mixin]

**Purpose and location.** Model description: Caching Utilities. Defined in `caching/models/caching_mixin.py:14` as an Odoo model; bases: `models.AbstractModel`. (model `caching.mixin`.) It is an abstract model: it has no table and is only mixed into others.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`"Caching Mixin"`) |

**Class attributes.** `_name = 'caching.mixin'`; `_description = 'Caching Utilities'`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `get_fs_stats`, `get_global_static_info`, `force_invalidate_cache`.

**See also:** test `test_service_only_methods_cannot_be_called_over_rpc (tests/test_rpc_exposure.py)`.

### `ResConfigSettings` [@ANCHOR: caching:caching_res_config_settings_res_config_settings]

**Purpose and location.** Extends the Odoo model "res.config.settings" with the fields `caching_safe_quota_mb`, `caching_invalidation_version`, `caching_pwa_theme_color`, `caching_pwa_background_color` and the methods `get_values`, `set_values`, `action_force_cache_invalidation`. Defined in `caching/models/res_config_settings.py:14` as an Odoo model; bases: `models.TransientModel`. (extends `res.config.settings`.) It is a transient (wizard) model: its rows are short-lived and removed by Odoo's vacuum.

| Field | Type | Meaning |
|---|---|---|
| `caching_safe_quota_mb` | `Integer` | Safe Quota (MB) |
| `caching_invalidation_version` | `Integer` | Cache Invalidation Version (readonly=`True`) |
| `caching_pwa_theme_color` | `Char` | mirrors the related field `"website_id.caching_pwa_theme_color"` (related=`"website_id.caching_pwa_theme_color"`; readonly=`False`) |
| `caching_pwa_background_color` | `Char` | mirrors the related field `"website_id.caching_pwa_background_color"` (related=`"website_id.caching_pwa_background_color"`; readonly=`False`) |

**Class attributes.** `_inherit = "res.config.settings"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `get_values`, `set_values`, `action_force_cache_invalidation`.

**See also:** test `test_01_quota_config_updates_sw (tests/test_settings_and_cache.py)`.

### `Website` [@ANCHOR: caching:caching_website_website]

**Purpose and location.** Extends the Odoo model "website" with the fields `caching_safe_quota_mb`, `caching_pwa_theme_color`, `caching_pwa_background_color`, `caching_invalidation_version`. Defined in `caching/models/website.py:6` as an Odoo model; bases: `models.Model`. (extends `website`.)

| Field | Type | Meaning |
|---|---|---|
| `caching_safe_quota_mb` | `Integer` | Safe Quota (MB): Maximum total size in MB of cached files. If total files exceed this, the max single file size cached will be lowered dynamically. (default=`35`) |
| `caching_pwa_theme_color` | `Char` | PWA Theme Color (default=`"#875A7B"`) |
| `caching_pwa_background_color` | `Char` | PWA Background Color (default=`"#ffffff"`) |
| `caching_invalidation_version` | `Integer` | Cache Invalidation Version: Increment this value to force users' browsers to immediately wipe their cache. (default=`1`) |

**Class attributes.** `_inherit = "website"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_01_quota_config_updates_sw (tests/test_settings_and_cache.py)`.
