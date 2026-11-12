# Data structures

### `ResConfigSettings` [@ANCHOR: advertising:advertising_res_config_settings_res_config_settings]

**Purpose and location.** Extends the Odoo model "res.config.settings" with the fields `google_adsense_client_id`, `google_adsense_footer_slot_id`, `google_adsense_sidebar_slot_id`. Defined in `advertising/models/res_config_settings.py:6` as an Odoo model; bases: `models.TransientModel`. (extends `res.config.settings`.) It is a transient (wizard) model: its rows are short-lived and removed by Odoo's vacuum.

| Field | Type | Meaning |
|---|---|---|
| `google_adsense_client_id` | `Char` | no label or help in the source (readonly=`False`) |
| `google_adsense_footer_slot_id` | `Char` | no label or help in the source (readonly=`False`) |
| `google_adsense_sidebar_slot_id` | `Char` | no label or help in the source (readonly=`False`) |

**Class attributes.** `_inherit = "res.config.settings"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_02_client_id_alone_loads_script_but_no_footer_slot (tests/test_advertising_layout.py)`.

### `Website` [@ANCHOR: advertising:advertising_website_website]

**Purpose and location.** Extends the Odoo model "website" with the fields `google_adsense_client_id`, `google_adsense_footer_slot_id`, `google_adsense_sidebar_slot_id`. Defined in `advertising/models/website.py:6` as an Odoo model; bases: `models.Model`. (extends `website`.)

| Field | Type | Meaning |
|---|---|---|
| `google_adsense_client_id` | `Char` | AdSense Publisher ID: Google AdSense publisher ID (e.g. ca-pub-XXXXXXXXXXXXXXXX). Leave empty to serve no ads at all -- this is the default, and nothing in the site layout renders any ad-related markup or script until this is set. |
| `google_adsense_footer_slot_id` | `Char` | AdSense Footer Ad Slot ID: The AdSense ad-unit slot ID for the single footer banner placement (docs/proposals/ADVERTISING.md's recommended minimal starting point). Both this and the Publisher ID above must be set for the footer ad to render -- setting only one renders nothing. |
| `google_adsense_sidebar_slot_id` | `Char` | AdSense Sidebar Ad Slot ID: The AdSense ad-unit slot ID for a fixed-position sidebar placement, per Bruce's own 2026-09-01 direction ('a sidebar and footer per page'). Renders as a fixed skyscraper unit on the page's right edge on wide viewports only (hidden below the Bootstrap xl breakpoint, so it never competes with content on narrow screens). Both this and the Publisher ID above must be set for the sidebar ad to render -- setting only one renders nothing. |

**Class attributes.** `_inherit = "website"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_02_client_id_alone_loads_script_but_no_footer_slot (tests/test_advertising_layout.py)`.
