# Data structures

### `SiteBlogUrl` [@ANCHOR: user_websites:user_websites_blog_urls_site_blog_url]

**Purpose and location.** Stand-in for the stock page's `blog_url` (a QueryURL): the blog links it makes.  The stock blog post page asks `blog_url(blog=..., tag=..., date_begin=...)` for its breadcrumb, tag badges, archive and blog list, and answers /blog/<name>-<id>[/tag/...]. A member's blog has one address and no tag or archive pages, so every such link is the blog's own address (a blog that is not a member's keeps the stock one). Defined in `user_websites/controllers/blog_urls.py:36` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`, `__call__`.

**See also:** test `test_02_post_is_served_at_the_callsign_address (tests/test_user_blog_urls.py)`.

### `UserBlogPages` [@ANCHOR: user_websites:user_websites_blog_urls_user_blog_pages]

**Purpose and location.** The class has no docstring; from its definition it provides `_blog_owner`, `user_blog_post`, `user_blog_feed`. Defined in `user_websites/controllers/blog_urls.py:59` as a class; bases: `http.Controller`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_blog_owner`, `user_blog_post`, `user_blog_feed`.

**See also:** test `test_04_a_post_is_only_found_under_its_own_site (tests/test_user_blog_urls.py)`.

### `UserBlogRedirects` [@ANCHOR: user_websites:user_websites_blog_urls_user_blog_redirects]

**Purpose and location.** Odoo's own blog addresses, redirected for the blogs that have a /<callsign>/blog address.  A bare @http.route() keeps the stock route list and adds this check in front of it. Defined in `user_websites/controllers/blog_urls.py:183` as a class; bases: `WebsiteBlog`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `blog`, `blog_feed`, `old_blog_post`, `blog_post`.

**See also:** test `test_enqueue_failure_never_fails_create_or_write (tests/test_blog_post_no_website_request.py)`.

### `UserWebsitesController` [@ANCHOR: user_websites:user_websites_main_user_websites_controller]

**Purpose and location.** The class has no docstring; from its definition it provides `report_violation_form`, `_render_report_page`, `report_violation`, `_report_refusal`, `user_blog_index`, `user_home_legacy_redirect`, `user_home_fallback`, `create_site`. Defined in `user_websites/controllers/main.py:301` as a class; bases: `http.Controller`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `report_violation_form`, `_render_report_page`, `report_violation`, `_report_refusal`, `user_blog_index`, `user_home_legacy_redirect`, `user_home_fallback`, `create_site`, `create_blog`, `create_blog_post`, `_get_own_blog_post_for_edit`, `blog_post_edit`, `_render_blog_post_editor`, `blog_post_edit_submit`, `documentation`, `community_directory`, `privacy_dashboard`, `privacy_export`, `privacy_export_zip`, `privacy_delete_content`, `privacy_erased`, `submit_appeal`, `pending_reports`, `subscribe`, `unsubscribe`.

**See also:** test `test_01_api_pending_reports_admin_access (tests/test_controllers.py)`.

### `UserWebsitesOwnerController` [@ANCHOR: user_websites:user_websites_owner_site_user_websites_owner_controller]

**Purpose and location.** The class has no docstring; from its definition it provides `_svc`, `_resolve_site`, `_redirect_legacy_slug`, `_owner_domain`, `_rate_limit`, `_qcontext`, `_report_if_hostile`, `_clean_body`. Defined in `user_websites/controllers/owner_site.py:176` as a class; bases: `http.Controller`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_svc`, `_resolve_site`, `_redirect_legacy_slug`, `_owner_domain`, `_rate_limit`, `_qcontext`, `_report_if_hostile`, `_clean_body`, `owner_site_entry`, `owner_site_dashboard`, `_own_page`, `owner_page_new`, `owner_page_edit`, `owner_page_save`, `_editor_with_error`, `_back_to_editor`, `owner_page_publish`, `owner_post_publish`, `_own_post`, `_own_page_any`, `_back`, `owner_delete_confirm`, `owner_delete`, `owner_restore`, `_attachment_owner` and more.

**See also:** test `test_05_owner_can_create_blog_post (tests/test_user_websites_blog_post.py)`.

### `UserWebsitesApi` [@ANCHOR: user_websites:user_websites_user_websites_api_user_websites_api]

**Purpose and location.** The class has no docstring; from its definition it provides `api_domains`. Defined in `user_websites/controllers/user_websites_api.py:12` as a class; bases: `http.Controller`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `api_domains`.

**See also:** test `test_domains_api_returns_all_domains (tests/test_domains_api.py)`.

### `BlogBlog` [@ANCHOR: user_websites:user_websites_blog_blog_blog_blog]

**Purpose and location.** Extends the Odoo model ["blog.blog", "user_websites.owned.mixin"] with the fields `name`, `site_path` and the methods `_compute_site_path`, `_check_blog_quota`, `create`, `check_access`, `write`, `unlink`. Defined in `user_websites/models/blog_blog.py:8` as an Odoo model; bases: `models.Model`. (model `blog.blog`; inherits ["blog.blog", "user_websites.owned.mixin"].)

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `site_path` | `Char` | Public Address: /<callsign>/blog for a member's or group's blog; empty for any other blog. (compute=`"_compute_site_path"`) |

**Class attributes.** `_name = "blog.blog"`; `_inherit = ["blog.blog", "user_websites.owned.mixin"]`; `_name_owner_uniq = models.Constraint("UNIQUE(name, owner_user_id, user_websites_group_id)", "You already have a blog wi`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_compute_site_path`, `_check_blog_quota`, `create`, `check_access`, `write`, `unlink`.

**See also:** test `test_01_blog_creation_is_blocked_past_the_configured_limit (tests/test_blog_limits.py)`.

### `BlogPost` [@ANCHOR: user_websites:user_websites_blog_post_blog_post]

**Purpose and location.** Extends the Odoo model ["blog.post", "user_websites.owned.mixin"] with the fields `name`, `view_count` and the methods `_compute_website_url`, `_invalidate_cloudflare_cache`, `_get_blog_urls`, `_check_blog_post_quota`, `_sanitize_member_content`, `create`. Defined in `user_websites/models/blog_post.py:16` as an Odoo model; bases: `models.Model`. (model `blog.post`; inherits ["blog.post", "user_websites.owned.mixin"].)

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `view_count` | `Integer` | View Count: Privacy-friendly tracking of post views. (default=`0`) |

**Class attributes.** `_name = "blog.post"`; `_inherit = ["blog.post", "user_websites.owned.mixin"]`; `_name_owner_uniq = models.Constraint("UNIQUE(name, owner_user_id, user_websites_group_id)", "You already have a blog po`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_compute_website_url`, `_invalidate_cloudflare_cache`, `_get_blog_urls`, `_check_blog_post_quota`, `_sanitize_member_content`, `create`, `check_access`, `write`, `unlink`, `send_weekly_digest`.

**See also:** test `test_01_blog_creation_is_blocked_past_the_configured_limit (tests/test_blog_limits.py)`.

### `ContentViolationAppeal` [@ANCHOR: user_websites:user_websites_content_violation_appeal_content_violation_appeal]

**Purpose and location.** Model description: User Website Moderation Appeal. Defined in `user_websites/models/content_violation_appeal.py:8` as an Odoo model; bases: `models.Model`. (model `content.violation.appeal`; inherits ["mail.thread", "mail.activity.mixin"]; default order "create_date desc".)

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `user_id` | `Many2one` | Suspended User (comodel `res.users`; required=`False`; default=`False`; ondelete=`"cascade"`) |
| `group_id` | `Many2one` | Suspended Group (comodel `user.websites.group`; required=`False`; default=`False`; ondelete=`"cascade"`) |
| `reason` | `Text` | Appeal Reason (required=`True`) |
| `company_id` | `Many2one` | Company (comodel `res.company`; required=`True`; default=`lambda self: self.env.company`) |
| `state` | `Selection` | Status (values `new`, `approved`, `rejected`; default=`"new"`; index=`True`) |

**Class attributes.** `_name = "content.violation.appeal"`; `_description = "User Website Moderation Appeal"`; `_inherit = ["mail.thread", "mail.activity.mixin"]`; `_order = "create_date desc"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_check_appeal_target`, `action_approve`, `action_reject`.

**See also:** test `test_03_settings_save_never_touches_group_membership_anymore (tests/test_config_settings.py)`.

### `ContentViolationReportModeration` [@ANCHOR: user_websites:user_websites_content_violation_report_moderation_content_violation_report_moderation]

**Purpose and location.** Extends the Odoo model "content.violation.report" with the fields `content_group_id` and the methods `_cron_notify_pending_reports`, `_increment_strike_count`, `_apply_enforcement_action`. Defined in `user_websites/models/content_violation_report_moderation.py:34` as an Odoo model; bases: `models.Model`. (extends `content.violation.report`.)

| Field | Type | Meaning |
|---|---|---|
| `content_group_id` | `Many2one` | Content Group (comodel `user.websites.group`; ondelete=`"set null"`) |

**Class attributes.** `_inherit = "content.violation.report"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_cron_notify_pending_reports`, `_increment_strike_count`, `_apply_enforcement_action`.

**See also:** test `test_04_group_moderation_cascading_strikes (tests/test_moderation.py)`.

### `HamGdprExportToken` [@ANCHOR: user_websites:user_websites_ham_gdpr_export_token_ham_gdpr_export_token]

**Purpose and location.** Model description: Short-lived, single-use token authorizing one GDPR CSV/zip export download. Defined in `user_websites/models/ham_gdpr_export_token.py:28` as an Odoo model; bases: `models.Model`. (model `ham.gdpr.export.token`.) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `user_id` | `Many2one` | Requesting User (comodel `res.users`; required=`True`; index=`True`; ondelete=`"cascade"`) |
| `token` | `Char` | Token (required=`True`; index=`True`; copy=`False`) |
| `consumed` | `Boolean` | Consumed (default=`False`) |
| `name` | `Char` | Name (compute=`"_compute_name"`; store=`True`) |

**Class attributes.** `_name = "ham.gdpr.export.token"`; `_description = "Short-lived, single-use token authorizing one GDPR CSV/zip export download"`; `_token_unique = models.Constraint(
        "UNIQUE(token)", "Export tokens must be unique."
    )`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_compute_name`, `create_for_current_user`, `_consume`, `consume_and_export`.

**See also:** test `test_01b_prevent_public_user_create_via_self_ownership (tests/test_orm_security.py)`.

### `SiteSlugConverter` [@ANCHOR: user_websites:user_websites_ir_http_site_slug_converter]

**Purpose and location.** The class has no docstring; from its definition it provides `to_python`. Defined in `user_websites/models/ir_http.py:22` as a class; bases: `UnicodeConverter`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `to_python`.

**See also:** test `test_09_reserved_slugs_cover_static_single_segment_routes (tests/test_member_site_home_url.py)`.

### `IrHttp` [@ANCHOR: user_websites:user_websites_ir_http_ir_http]

**Purpose and location.** Extends the Odoo model "ir.http" with no new fields and the methods `_get_converters`, `_serve_page`. Defined in `user_websites/models/ir_http.py:33` as an Odoo model; bases: `models.AbstractModel`. (extends `ir.http`.) It is an abstract model: it has no table and is only mixed into others.

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "ir.http"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_get_converters`, `_serve_page`.

**See also:** test `test_01_home_served_at_slug (tests/test_member_site_home_url.py)`.

### `ResConfigSettings` [@ANCHOR: user_websites:user_websites_res_config_settings_res_config_settings]

**Purpose and location.** Extends the Odoo model "res.config.settings" with the fields `global_website_page_limit`, `company_abuse_email`. Defined in `user_websites/models/res_config_settings.py:7` as an Odoo model; bases: `models.TransientModel`. (extends `res.config.settings`.) It is a transient (wizard) model: its rows are short-lived and removed by Odoo's vacuum.

| Field | Type | Meaning |
|---|---|---|
| `global_website_page_limit` | `Integer` | Global Page Limit: Default maximum number of web pages a standard user can create. (default=`100`) |
| `company_abuse_email` | `Char` | Abuse Reporting Email: Email address where content violation reports will be sent. |

**Class attributes.** `_inherit = "res.config.settings"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_03_settings_save_never_touches_group_membership_anymore (tests/test_config_settings.py)`.

### `ResUsers` [@ANCHOR: user_websites:user_websites_res_users_res_users]

**Purpose and location.** Inherits from `res.users` to add features for personal user websites. Defined in `user_websites/models/res_users.py:112` as an Odoo model; bases: `models.Model`. (inherits ["res.users", "edge.routing.mixin"].)

| Field | Type | Meaning |
|---|---|---|
| `website_slug` | `Char` | Website Slug: The URL-friendly identifier for the user's site. Alphanumeric and hyphens only. (index=`"trigram"`) |
| `website_page_limit` | `Integer` | Website Page Limit: Maximum number of pages this user can create. If 0, the global limit is used. |
| `privacy_show_in_directory` | `Boolean` | Show in Public Directory: If checked, a link to this user's website will appear in the public community directory. (default=`False`) |
| `appeal_ids` | `One2many` | Moderation Appeals (comodel `content.violation.appeal`) |

**Class attributes.** `_inherit = ["res.users", "edge.routing.mixin"]`; `_website_slug_format = models.Constraint("CHECK(website_slug IS NULL OR website_slug = '' OR website_slug ~ '^[a-z0-9\\-]+$`; `_website_slug_unique = models.Constraint("UNIQUE(website_slug)", "The Website Slug must be unique!")`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_register_hook`, `SELF_WRITEABLE_FIELDS`, `_user_websites_menu_pages`, `_check_reserved_slugs`, `_is_admin`, `_get_user_id_by_slug`, `create`, `_slug_identity_fields`, `_slug_identity`, `_apply_slug_policy`, `write`, `_get_page_limit`, `_get_blog_limit`, `_get_blog_post_limit`, `_get_gdpr_streamed_keys`, `_get_gdpr_export_data`, `_execute_gdpr_erasure`.

**See also:** test `test_01_blog_creation_is_blocked_past_the_configured_limit (tests/test_blog_limits.py)`.

### `ResUsersModeration` [@ANCHOR: user_websites:user_websites_res_users_moderation_res_users_moderation]

**Purpose and location.** Feature-specific extension of res.users to handle the Three-Strikes moderation, suspension logic, and high-performance slug caching. Defined in `user_websites/models/res_users_moderation.py:11` as an Odoo model; bases: `models.Model`. (extends `res.users`.)

| Field | Type | Meaning |
|---|---|---|
| `violation_strike_count` | `Integer` | Violation Strikes: Number of upheld content violations. Hitting 3 triggers an automatic suspension. (default=`0`) |
| `is_suspended_from_websites` | `Boolean` | Suspended from Websites: If True, all personal pages and blogs are forcefully unpublished and locked. (default=`False`) |
| `suspended_group_ids` | `Many2many` | Suspended Groups (comodel `user.websites.group`; compute=`"_compute_suspended_group_ids"`) |

**Class attributes.** `_inherit = "res.users"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_compute_suspended_group_ids`, `action_suspend_user_websites`, `action_pardon_user_websites`.

**See also:** test `test_05_community_directory_excludes_suspended_user (tests/test_controllers.py)`.

### `ResUsersSiteEligibility` [@ANCHOR: user_websites:user_websites_site_eligibility_res_users_site_eligibility]

**Purpose and location.** Extends the Odoo model "res.users" with the fields `website_ineligible` and the methods `_website_eligibility_gaps`, `_hams_website_eligible`, `_website_publicly_visible`, `_website_site_user_ids`, `reconcile_website_eligibility`, `_hide_site_content`. Defined in `user_websites/models/site_eligibility.py:36` as an Odoo model; bases: `models.Model`. (extends `res.users`.)

| Field | Type | Meaning |
|---|---|---|
| `website_ineligible` | `Boolean` | Website Not Allowed: The last reconciled answer of _website_eligibility_gaps(): True while the member's site is hidden because a requirement is missing. Maintained by reconcile_website_eligibility(); never set by the member. (default=`False`; index=`True`; readonly=`True`; copy=`False`) |

**Class attributes.** `_inherit = "res.users"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_website_eligibility_gaps`, `_hams_website_eligible`, `_website_publicly_visible`, `_website_site_user_ids`, `reconcile_website_eligibility`, `_hide_site_content`, `_show_site_content`, `_cron_reconcile_website_eligibility`.

**See also:** test `test_service_only_methods_cannot_be_called_over_rpc (tests/test_rpc_exposure.py)`.

### `UserWebsitesPublicDirectoryView` [@ANCHOR: user_websites:user_websites_sql_views_user_websites_public_directory_view]

**Purpose and location.** Model description: Community Directory SQL View. Defined in `user_websites/models/sql_views.py:8` as an Odoo model; bases: `models.Model`. (model `user_websites.public_directory_view`.) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `user_id` | `Many2one` | User (comodel `res.users`; readonly=`True`) |
| `name` | `Char` | Name (readonly=`True`) |
| `website_slug` | `Char` | Slug (readonly=`True`) |
| `view_count` | `Integer` | Total Views (readonly=`True`) |
| `write_date` | `Datetime` | Last Updated (readonly=`True`) |

**Class attributes.** `_name = "user_websites.public_directory_view"`; `_description = "Community Directory SQL View"`; `_auto = False`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `init`.

**See also:** test `test_create_with_request_lacking_website_attribute (tests/test_blog_post_no_website_request.py)`.

### `UserWebsitesContentRoutingView` [@ANCHOR: user_websites:user_websites_sql_views_user_websites_content_routing_view]

**Purpose and location.** Model description: Content Routing SQL View. Defined in `user_websites/models/sql_views.py:61` as an Odoo model; bases: `models.Model`. (model `user_websites.content_routing_view`.) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `res_model` | `Char` | Model (readonly=`True`) |
| `res_id` | `Integer` | Resource ID (readonly=`True`) |
| `website_slug` | `Char` | Slug (readonly=`True`) |

**Class attributes.** `_name = "user_websites.content_routing_view"`; `_description = "Content Routing SQL View"`; `_auto = False`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `init`.

**See also:** test `test_create_with_request_lacking_website_attribute (tests/test_blog_post_no_website_request.py)`.

### `UserWebsitesWeeklyDigestView` [@ANCHOR: user_websites:user_websites_sql_views_user_websites_weekly_digest_view]

**Purpose and location.** Model description: Weekly Digest SQL View. Defined in `user_websites/models/sql_views.py:133` as an Odoo model; bases: `models.Model`. (model `user_websites.weekly_digest_view`.) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `partner_id` | `Many2one` | Subscriber (comodel `res.partner`; readonly=`True`) |
| `author_name` | `Char` | Author (readonly=`True`) |
| `owner_model` | `Char` | Owner Model (readonly=`True`) |
| `owner_record_id` | `Integer` | Owner ID (readonly=`True`) |
| `post_ids_string` | `Char` | Post IDs (readonly=`True`) |
| `first_post_id` | `Integer` | First Post ID (readonly=`True`) |

**Class attributes.** `_name = "user_websites.weekly_digest_view"`; `_description = "Weekly Digest SQL View"`; `_auto = False`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `init`.

**See also:** test `test_01_weekly_digest_and_unsubscribe_headers (tests/test_subscriptions.py)`.

### `UserWebsitesDbFunctions` [@ANCHOR: user_websites:user_websites_sql_views_user_websites_db_functions]

**Purpose and location.** Model description: User Websites DB Functions. Defined in `user_websites/models/sql_views.py:192` as an Odoo model; bases: `models.AbstractModel`. (model `user_websites.db_functions`.) It is an abstract model: it has no table and is only mixed into others.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |

**Class attributes.** `_name = "user_websites.db_functions"`; `_description = "User Websites DB Functions"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `init`.

**See also:** test `test_helper_functions_are_exercised (tests/test_owner_site.py)`.

### `UserWebsitesGroup` [@ANCHOR: user_websites:user_websites_user_websites_groups_user_websites_group]

**Purpose and location.** Represents a group of users who can manage a shared website. Defined in `user_websites/models/user_websites_groups.py:173` as an Odoo model; bases: `models.Model`. (model `user.websites.group`; inherits ["mail.thread", "mail.activity.mixin", "edge.routing.mixin"].)

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Group Name (required=`True`) |
| `violation_strike_count` | `Integer` | Violation Strikes: Number of upheld content violations. Hitting 3 triggers an automatic suspension. (default=`0`) |
| `is_suspended_from_websites` | `Boolean` | Suspended from Websites: If true, this group's websites are unpublished and they cannot manage content. (default=`False`; readonly=`True`) |
| `appeal_ids` | `One2many` | Moderation Appeals: Appeals submitted on behalf of this group. (comodel `content.violation.appeal`) |
| `odoo_group_id` | `Many2one` | Linked Odoo Group: The Odoo security group associated with this website. (comodel `res.groups`; required=`True`; ondelete=`"cascade"`) |
| `member_ids` | `Many2many` | Group Members: Users who have editing rights for this group site. (comodel `res.users`; related=`"odoo_group_id.user_ids"`; readonly=`False`) |
| `website_page_ids` | `One2many` | Group Pages: Pages belonging to this group website. (comodel `website.page`) |
| `blog_post_ids` | `One2many` | Group Blog Posts: Blog posts belonging to this group. (comodel `blog.post`) |
| `blog_blog_ids` | `One2many` | Group Blogs (comodel `blog.blog`) |
| `company_id` | `Many2one` | Company (comodel `res.company`; required=`True`; default=`lambda self: self.env.company`) |

**Class attributes.** `_name = "user.websites.group"`; `_description = "User Websites Group"`; `_inherit = ["mail.thread", "mail.activity.mixin", "edge.routing.mixin"]`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_user_websites_menu_pages`, `_check_reserved_slugs`, `create`, `write`, `action_suspend_group_websites`, `action_pardon_group_websites`.

**See also:** test `test_01_blog_creation_is_blocked_past_the_configured_limit (tests/test_blog_limits.py)`.

### `UserWebsitesOwnedMixin` [@ANCHOR: user_websites:user_websites_user_websites_owned_mixin_user_websites_owned_mixin]

**Purpose and location.** An abstract mixin that provides the 'Proxy Ownership' pattern to any model. Inherit this to securely tie a model to a user or a group website. Defined in `user_websites/models/user_websites_owned_mixin.py:11` as an Odoo model; bases: `models.AbstractModel`. (model `user_websites.owned.mixin`.) It is an abstract model: it has no table and is only mixed into others.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `owner_user_id` | `Many2one` | Owner: The user who 'owns' this record business-wise. (comodel `res.users`; index=`True`; ondelete=`"cascade"`) |
| `user_websites_group_id` | `Many2one` | Group Owner: The group that owns this record. (comodel `user.websites.group`; index=`True`; ondelete=`"cascade"`) |
| `hidden_by_eligibility` | `Boolean` | Hidden While Ineligible: Unpublished by res.users.reconcile_website_eligibility() because the owner may not have a site right now; republished when they may again. Never set on a piece the owner unpublished themselves. (default=`False`; index=`True`; copy=`False`) |
| `deleted_by_owner` | `Datetime` | Deleted By Owner: When the owner pressed Delete. The record is kept, archived, and hidden from every public surface and from the owner's lists; the owner can restore it. The platform retains data, so this is a removal from view, never an erasure. (index=`True`; copy=`False`) |

**Class attributes.** `_name = "user_websites.owned.mixin"`; `_description = "User Websites Proxy Ownership Mixin"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_user_site_slugs`, `_user_websites_navbar_profile_user`, `_check_proxy_ownership_create`, `_check_proxy_ownership_write`.

**See also:** test `test_01_blog_creation_is_blocked_past_the_configured_limit (tests/test_blog_limits.py)`.

### `Website` [@ANCHOR: user_websites:user_websites_website_website]

**Purpose and location.** Extends the Odoo model "website" with no new fields and the methods `_enumerate_pages`. Defined in `user_websites/models/website.py:10` as an Odoo model; bases: `models.Model`. (extends `website`.)

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "website"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_enumerate_pages`.

**See also:** test `test_04_portal_owner_cannot_set_website_id_to_opt_out_of_isolation (tests/test_blog_limits.py)`.

### `WebsitePage` [@ANCHOR: user_websites:user_websites_website_page_website_page]

**Purpose and location.** Extends the Odoo model ["website.page", "user_websites.owned.mixin"] with the fields `name`, `view_count`, `deleted_original_url` and the methods `_serve_page`, `_invalidate_cloudflare_cache`, `_sanitize_user_arch`, `_trigger_malicious_arch_violation`, `_malicious_arch_violation_targets`, `_file_malicious_arch_report`. Defined in `user_websites/models/website_page.py:52` as an Odoo model; bases: `models.Model`. (model `website.page`; inherits ["website.page", "user_websites.owned.mixin"].)

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `view_count` | `Integer` | View Count: Privacy-friendly tracking of page views. (default=`0`) |
| `deleted_original_url` | `Char` | Address Before Deletion: Set when the owner deletes the page: the page is moved to a private address under the site so its old address can be used again, and restored to this one when possible. (copy=`False`) |

**Class attributes.** `_name = "website.page"`; `_inherit = ["website.page", "user_websites.owned.mixin"]`; `_url_website_uniq = models.Constraint("UNIQUE(url, website_id)", "The page URL must be unique per website!")`; `_FLUSH_DECREMENT_LUA = "local v = redis.call('DECRBY', KEYS[1], ARGV[1]) "
        "if v <= 0 then redis.call('DEL', KEYS[1`; `_FLUSH_BATCH_KEYS = 1000`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_serve_page`, `_invalidate_cloudflare_cache`, `_sanitize_user_arch`, `_trigger_malicious_arch_violation`, `_malicious_arch_violation_targets`, `_file_malicious_arch_report`, `_get_page_id_by_url`, `create`, `check_access`, `write`, `unlink`, `_flush_redis_view_counters`.

**See also:** test `test_01_blog_creation_is_blocked_past_the_configured_limit (tests/test_blog_limits.py)`.

### `SiteResponse` [@ANCHOR: user_websites:user_websites_controllers_owner_site_site_response]

**Purpose and location.** The exception that lets code deep inside an owner-site request end the request with a response of its own: a redirect from a retired slug, or the "what is missing" page shown to a member who is not yet eligible. Defined in `user_websites/controllers/owner_site.py`; raised by `_resolve_site()` and caught by the `answers_site_response` route decorator.

**Definition.** `class SiteResponse(Exception)`, built as `SiteResponse(response)`.

| Attribute | Type | Meaning |
|---|---|---|
| `response` | a werkzeug/Odoo response | The complete response the route must return. Set once in `__init__`; the exception message is the fixed text `owner site response`. |

**Why it is not an `HTTPException`.** Odoo's frontend error handler turns any raised `HTTPException` into its own error page (a 301 becomes a 418) and throws the response away. A plain `Exception` subclass passes through until `answers_site_response` catches it, so the response arrives unchanged.

**Invariants and concurrency.** It is raised and caught inside one request on one thread; it is never stored or sent. A route that is not wrapped by `answers_site_response` would let it escape as a server error, so a route that can reach `_resolve_site()` must carry the decorator.

**Functions that touch it.** `SiteResponse.__init__`, `_resolve_site`, `answers_site_response`.

**See also:** tests `TestSiteResponse` in `user_websites/tests/test_site_response.py` (carries the response, is not an `HTTPException`, the decorator returns it and lets other errors through) and `test_03_owner_is_told_what_is_missing_and_cannot_create` in `user_websites/tests/test_site_eligibility_hook.py` (the real 403 page).
