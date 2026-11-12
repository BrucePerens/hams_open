# Data structures

### `DatabaseManagerLockdown` [@ANCHOR: hams_base:hams_base_database_lockdown_database_manager_lockdown]

**Purpose and location.** Every `/web/database/*` route is a 404 outside developer mode.  Each override keeps the route (`type`, `auth`, `methods`) of the method it replaces. Defined in `hams_base/controllers/database_lockdown.py:56` as a class; bases: `Database`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_refuse`, `selector`, `manager`, `create`, `duplicate`, `drop`, `backup`, `restore`, `change_password`, `list`.

**See also:** test `test_notification_email_layout_renders_without_env_keyerror (tests/test_mail_templates.py)`.

### `HamsJsonRPCDispatcher` [@ANCHOR: hams_base:hams_base_database_lockdown_hams_json_rpcdispatcher]

**Purpose and location.** The class has no docstring; from its definition it provides `handle_error`. Defined in `hams_base/controllers/database_lockdown.py:114` as a class; bases: `JsonRPCDispatcher`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `handle_error`.

**See also:** test `test_the_dispatchers_replace_the_core_ones (tests/test_database_lockdown.py)`.

### `HamsJson2Dispatcher` [@ANCHOR: hams_base:hams_base_database_lockdown_hams_json2_dispatcher]

**Purpose and location.** The class has no docstring; from its definition it provides `handle_error`. Defined in `hams_base/controllers/database_lockdown.py:138` as a class; bases: `Json2Dispatcher`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `handle_error`.

**See also:** test `test_the_dispatchers_replace_the_core_ones (tests/test_database_lockdown.py)`.

### `EmailPolicyController` [@ANCHOR: hams_base:hams_base_email_policy_email_policy_controller]

**Purpose and location.** The class has no docstring; from its definition it provides `email_policy`. Defined in `hams_base/controllers/email_policy.py:4` as a class; bases: `http.Controller`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `email_policy`.

**See also:** test `test_email_policy_page_reachable (tests/test_unsubscribe.py)`.

### `UnsubscribeController` [@ANCHOR: hams_base:hams_base_unsubscribe_unsubscribe_controller]

**Purpose and location.** The class has no docstring; from its definition it provides `unsubscribe_page`, `unsubscribe_lockout`. Defined in `hams_base/controllers/unsubscribe.py:6` as a class; bases: `http.Controller`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `unsubscribe_page`, `unsubscribe_lockout`.

**See also:** test `test_public_user_post_to_lockout_is_rejected_by_auth_user (tests/test_unsubscribe.py)`.

### `DmarcReport` [@ANCHOR: hams_base:hams_base_dmarc_report_dmarc_report]

**Purpose and location.** Model description: DMARC RUA Report. Defined in `hams_base/models/dmarc_report.py:51` as an Odoo model; bases: `models.Model`. (model `hams_base.dmarc.report`; inherits ["mail.thread"]; default order "date_range_begin desc".)

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`"New Report"`) |
| `org_name` | `Char` | Organization |
| `email` | `Char` | Contact Email |
| `report_id` | `Char` | Report ID (index=`True`) |
| `date_range_begin` | `Datetime` | Begin Date |
| `date_range_end` | `Datetime` | End Date |
| `domain` | `Char` | Domain (index=`True`) |
| `adkim` | `Char` | ADKIM |
| `aspf` | `Char` | ASPF |
| `p` | `Char` | Policy (p) |
| `sp` | `Char` | Subdomain Policy (sp) |
| `pct` | `Integer` | Percentage |
| `record_ids` | `One2many` | Records (comodel `hams_base.dmarc.record`) |

**Class attributes.** `_name = "hams_base.dmarc.report"`; `_description = "DMARC RUA Report"`; `_inherit = ["mail.thread"]`; `_order = "date_range_begin desc"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `message_new`, `process_dmarc_attachment`, `_parse_dmarc_xml`.

**See also:** test `test_notification_email_layout_renders_without_env_keyerror (tests/test_mail_templates.py)`.

### `DmarcRecord` [@ANCHOR: hams_base:hams_base_dmarc_report_dmarc_record]

**Purpose and location.** Model description: DMARC RUA Record Details. Defined in `hams_base/models/dmarc_report.py:274` as an Odoo model; bases: `models.Model`. (model `hams_base.dmarc.record`.) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`"Record"`) |
| `report_id` | `Many2one` | Report (comodel `hams_base.dmarc.report`; required=`True`; ondelete=`"cascade"`) |
| `source_ip` | `Char` | Source IP (index=`True`) |
| `count` | `Integer` | Message Count |
| `disposition` | `Char` | Disposition (none, quarantine, reject) |
| `dkim_alignment` | `Char` | DKIM Alignment (pass/fail) |
| `spf_alignment` | `Char` | SPF Alignment (pass/fail) |

**Class attributes.** `_name = "hams_base.dmarc.record"`; `_description = "DMARC RUA Record Details"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_dmarc_record_tree_view_renders (tests/test_view_rendering_audit_cluster.py)`.

### `IrHttp` [@ANCHOR: hams_base:hams_base_ir_http_ir_http]

**Purpose and location.** Extends the Odoo model "ir.http" with no new fields and the methods `_match`, `_url_localized`, `_url_for`. Defined in `hams_base/models/ir_http.py:11` as an Odoo model; bases: `models.AbstractModel`. (extends `ir.http`.) It is an abstract model: it has no table and is only mixed into others.

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "ir.http"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_match`, `_url_localized`, `_url_for`.

**See also:** test `test_homepage_with_deleted_user_session_is_public (tests/test_stale_session_match.py)`.

### `MailThread` [@ANCHOR: hams_base:hams_base_mail_thread_mail_thread]

**Purpose and location.** Extends the Odoo model 'mail.thread' with no new fields and the methods `message_route`. Defined in `hams_base/models/mail_thread.py:9` as an Odoo model; bases: `models.AbstractModel`. (extends `mail.thread`.) It is an abstract model: it has no table and is only mixed into others.

**Fields.** none declared by this class.

**Class attributes.** `_inherit = 'mail.thread'`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `message_route`.

**See also:** test `test_vacation_reply_dropped (tests/test_mail_thread.py)`.

### `ResConfigSettings` [@ANCHOR: hams_base:hams_base_res_config_settings_res_config_settings]

**Purpose and location.** Extends the Odoo model 'res.config.settings' with the fields `compliance_org_name`, `compliance_mailing_address`, `dns_spf_record`, `enable_dmarc_instructions`, `dns_dmarc_record` and the methods `_compute_dns_records`. Defined in `hams_base/models/res_config_settings.py:4` as an Odoo model; bases: `models.TransientModel`. (extends `res.config.settings`.) It is a transient (wizard) model: its rows are short-lived and removed by Odoo's vacuum.

| Field | Type | Meaning |
|---|---|---|
| `compliance_org_name` | `Char` | Leave empty to use the company name (Settings > Companies), the single source of the site's name. |
| `compliance_mailing_address` | `Char` | no label or help in the source |
| `dns_spf_record` | `Text` | no label or help in the source (compute=`"_compute_dns_records"`) |
| `enable_dmarc_instructions` | `Boolean` | Disable if using AWS SES or another provider that manages DMARC natively. (default=`False`) |
| `dns_dmarc_record` | `Text` | no label or help in the source (compute=`"_compute_dns_records"`) |

**Class attributes.** `_inherit = 'res.config.settings'`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_compute_dns_records`.

**See also:** test `test_notification_email_layout_renders_without_env_keyerror (tests/test_mail_templates.py)`.

### `ResPartner` [@ANCHOR: hams_base:hams_base_res_partner_res_partner]

**Purpose and location.** Extends the Odoo model "res.partner" with no new fields and the methods `_message_receive_bounce`. Defined in `hams_base/models/res_partner.py:10` as an Odoo model; bases: `models.Model`. (extends `res.partner`.)

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "res.partner"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_message_receive_bounce`.

**See also:** test `test_a_members_bounce_notifies_their_parent_company_as_the_club (tests/test_res_partner_bounce.py)`.

### `ResUsers` [@ANCHOR: hams_base:hams_base_res_users_res_users]

**Purpose and location.** Extends the Odoo model "res.users" with no new fields and the methods `_normalize_login`, `_get_login_domain`, `create`, `write`. Defined in `hams_base/models/res_users.py:10` as an Odoo model; bases: `models.Model`. (extends `res.users`.)

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "res.users"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_normalize_login`, `_get_login_domain`, `create`, `write`.

**See also:** test `test_reset_password_resolves_one_user_for_any_case (tests/test_login_case_insensitive.py)`.

### `Website` [@ANCHOR: hams_base:hams_base_website_website]

**Purpose and location.** Extends the Odoo model "website" with no new fields and the methods `email_delivery_notice`. Defined in `hams_base/models/website.py:6` as an Odoo model; bases: `models.Model`. (extends `website`.)

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "website"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `email_delivery_notice`.

**See also:** test `test_email_delivery_notice_shows_only_when_configured (tests/test_unsubscribe.py)`.

### `WebsiteVisitor` [@ANCHOR: hams_base:hams_base_website_visitor_website_visitor]

**Purpose and location.** Extends the Odoo model "website.visitor" with no new fields and the methods `_handle_webpage_dispatch`, `_track_exists_for_this_request`. Defined in `hams_base/models/website_visitor.py:8` as an Odoo model; bases: `models.Model`. (extends `website.visitor`.)

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "website.visitor"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_handle_webpage_dispatch`, `_track_exists_for_this_request`.

**See also:** test `test_a_recent_track_for_the_same_url_skips_every_write (tests/test_visitor_track_dedup.py)`.
