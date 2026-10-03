# User Websites (`user_websites`)

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).*

Welcome to User Websites! This Odoo 19 module allows you to build and manage your own personal or group websites and blogs directly within Odoo. Whether you're a beginner or an expert, you'll find everything you need to create a professional online presence.

**Open Source Rule:** We built this for the open-source community. It runs perfectly on its own and does not rely on any proprietary code.

## 🌟 What Can I Do?

*   **Create Your Personal Site:** Get a unique URL (like `/yourname/home`) and use our easy drag-and-drop editor to build your pages.
*   **Start a Blog:** Share your thoughts and stories with the community. Everyone gets their own blog section.
*   **Collaborate with Groups:** Create shared websites for your team, club, or project.
*   **Join the Community:** Choose to show off your site in our public directory and discover what others are building.
*   **Control Your Privacy:** You decide what's public. We also provide full GDPR-compliant data export and deletion tools.

## 🚀 Getting Started

### 1. Initialize Your Site
The first time you visit your personal URL (e.g., click on your name in the navbar), you'll see a "Create Your Site" button. Click it to set up your initial layout.

### 2. Customize Your Pages
Once initialized, use the **Edit** button in the top right corner of any of your pages. You can drag and drop different "snippets" (blocks of content like text, images, or contact forms) onto your page.

### 3. Start Blogging
Visit your blog page (e.g., `/yourname/blog`) and click "Create Your Blog" to start posting. You can manage all your posts from this central location.

## 🛡️ Community & Safety

We want to keep our community safe and professional.
*   **Report Violations:** If you see content that breaks our rules, every personal or group website page (home page, blog, or blog post) shows a "Report Violation" button to visitors other than its owner. Our admins will review reports promptly.
*   **Moderation:** We use a 3-strike system. If a user repeatedly violates our community guidelines, their account may be suspended from using website features.
*   **Automated Security:** Our system automatically scans for and removes malicious code to protect all users.

## ⚙️ Configuration (For Administrators)

Go to **Settings > General Settings > User Websites** to configure the app.
* **Global Page Limit:** Set the default maximum number of pages a user is allowed to build (100 unless changed).
* **Administrators:** The "Manage Administrators" button opens Odoo's own Groups form for the User Websites "Administrator" group; add the users who are allowed to review abuse reports and manage all user and group websites there.
* **Abuse Reporting Email:** The address that receives the daily summary of pending violation reports (if empty, the company's own email is used).

## 🏗️ How It Works Under the Hood

We used a few neat tricks to make this secure and fast:

* **Just-In-Time Creation:** We don't waste database space creating empty blogs for users who never use them. The system only creates the website records when the owner first clicks "Create Your Site" (or "Create Your Blog") on their own not-yet-initialized page; until then, visitors see a placeholder.
* **Your Own Blog:** Each user or group gets its own Odoo `blog.blog` record (named `<Name>'s Blog`), owned by that user or group, so only its owner (or the group's members) and administrators can change it. A user may have up to 5 blogs by default.
* **Proxy Ownership:** Odoo normally only lets admins build web pages. We get around this securely. When a user creates a page, the system briefly logs in as a background Service Account to save the HTML to the database, but tags the user as the real "owner" so only they can edit it later.
* **Security Shield:** The module includes an automated sanitization engine that intercepts and neutralizes XSS and SSTI attempts. If a user tries to inject malicious code, the system strips it, files a violation report against the page's owner, and issues a strike.

---

# Technical Documentation

<system_role>
*Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).*

**Context:** Technical documentation strictly for LLMs and Integrators. Use this to build dependent modules without needing the source code.
</system_role>

---

<core_patterns>
## 1. 🏗️ Overview & Core Patterns
**Open Source Isolation Mandate:** This module is Open Source and available to the Odoo Community. It MUST NEVER be given dependencies on proprietary modules or anything else from the proprietary codebase.

The `user_websites` module enables decentralized content creation. It employs the **Proxy Ownership Pattern**: standard Odoo users cannot create `ir.ui.view` or `website.page` records due to core security. The module securely circumvents this by assigning an `owner_user_id`, evaluating custom Record Rules against it, and escalating privileges via a dedicated Service Account (`.with_user(svc_uid)`) strictly for the database write.
* **Ownership Validation:** Safely asserted by mixin create `[@ANCHOR: mixin_proxy_ownership_create]` and write `[@ANCHOR: mixin_proxy_ownership_write]` methods. Explicitly verified by `[@ANCHOR: test_mixin_ownership_validation]`.

* **Tenant Isolation:** Enforced via strict record rules verified by `[@ANCHOR: test_tenant_view_isolation]` and ACL overhead elimination to prevent log spam `[@ANCHOR: test_acl_overhead_loop_elimination]`.
* **Lazy JIT Provisioning:** Websites and Blogs do not exist upon user creation. They are provisioned Just-In-Time when the owner visits their slug root and triggers a POST request to `create_site`. This ensures explicit user consent to publish.
</core_patterns>

---

<data_model>
## 2. 🗄️ Data Model Reference

### Extended `res.users`
* **`website_slug`**: URL-safe identifier.
* **`privacy_show_in_directory`**: Opt-in for the public `/community` directory.
* **`violation_strike_count`**: Number of upheld content violations.
* **`is_suspended_from_websites`**: If True, all personal content is forcefully unpublished (the unpublishing itself is done in the background by `action_suspend_user_websites()`; a direct write of this field only invalidates the slug/page resolver caches) and the user's slug routes return 404.
* **`appeal_ids`** (`One2many`): Links to Moderation Appeals.

### Content Models (`website.page`, `blog.post`)
* **`owner_user_id`**: The proxy owner.
* **`user_websites_group_id`**: For shared group websites.
* **`view_count`**: Privacy-friendly server-side view tracker (the server counts non-admin page views in Redis and a cron flushes them to this column every 15 minutes). Only `website.page` counts are incremented today; this module declares `blog.post.view_count` but never increments it.

### Moderation Models
* **`content.violation.report`** (model itself now in `content_moderation`, this module's own extension in `models/content_violation_report_moderation.py`): Stores abuse reports. Originator is masked from the target owner. The system automatically generates a report and issues a strike if a user attempts to inject malicious SSTI/XSS payloads into their site architecture `[@ANCHOR: content_moderation:action_take_action_and_strike]` (via this module's own enforcement-hook override `[@ANCHOR: user_websites:COMM_apply_enforcement_action]`), tested by `[@ANCHOR: test_moderation_suspension]`. Admin spam is prevented via a daily digest cron (`ir_cron_notify_pending_reports` `[@ANCHOR: ir_cron_notify_pending_reports]`, `[@ANCHOR: cron_notify_pending_reports]`, verified by `[@ANCHOR: test_cron_pending_reports]`) and a session-guarded UI toast (`[@ANCHOR: toast_notifications_logic]`, `[@ANCHOR: admin_toast_logic]`, tested by `[@ANCHOR: test_tour_toast_notifications]`).

* **Security Auto-Moderation**: The `website.page` model includes `_sanitize_user_arch` `[@ANCHOR: website_page_sanitize_arch]`, verified by `[@ANCHOR: test_website_page_sanitize_arch]`, which forcefully removes `<script>`, `<iframe>`, `<object>`, `<embed>` and `<base>` elements and disables dangerous QWeb directives (`t-*` outside a fixed allowlist) and JS event handlers (`on*`) by renaming them to inert `data-blocked-*` attributes. It runs on `create()`/`write()` of page content by callers who are neither administrators nor the module's service account.
* **`content.violation.appeal`**: Used by suspended users to petition for account restoration.
</data_model>

---

<public_api>
## 3. 🐍 Public API & Extensibility Methods

### Explicit Dropzones
To prevent monolithic entanglement, `user_websites` provides the following explicitly designated dropzones. You MUST use `<xpath>` targeting these specific IDs and cite the corresponding Semantic Anchor:
* **Home Header:** `id="user_websites_dropzone_home_header"` -> `[@ANCHOR: dropzone_home_header]`

* **Home Footer:** `id="user_websites_dropzone_home_footer"` -> `[@ANCHOR: dropzone_home_footer]`

* **Global Navbar:** `id="user_websites_dropzone_navbar"` -> `[@ANCHOR: dropzone_navbar]`

* **Portal Templates:** `id="user_websites_dropzone_templates"` -> `[@ANCHOR: dropzone_templates]`

* **Snippets Sidebar:** `id="user_websites_dropzone_snippets"` -> `[@ANCHOR: dropzone_snippets]`

* **Website Layout:** `id="user_websites_dropzone_layout"` -> `[@ANCHOR: dropzone_layout]`

* **User Settings:** `id="user_websites_dropzone_users"` -> `[@ANCHOR: dropzone_users]`

* **Blog Post Form:** `id="user_websites_dropzone_blog_post"` -> `[@ANCHOR: dropzone_blog_post]`

* **Navbar Actions:** `id="user_websites_dropzone_navbar_actions"` -> `[@ANCHOR: dropzone_navbar_actions]`

* **Directory Card:** `id="user_websites_dropzone_directory_card"` -> `[@ANCHOR: dropzone_directory_card]`

* **Global Settings Form:** -> `[@ANCHOR: dropzone_settings]`

### Prohibited Dropzones
DO NOT USE user_websites_settings_dropzone. All settings views must now inherit directly from base.res_config_settings_view_form and target the //form element using the modernized `<app>`, `<block>`, and `<setting>` XML tags.

### Endpoints & Webhooks
* **Community Directory:** Renders public pages `/community` `[@ANCHOR: UX_COMMUNITY_DIRECTORY]`.

* **Violation Reporting:** Form endpoint `/website/report_violation` `[@ANCHOR: user_websites:UX_REPORT_VIOLATION]`, `[@ANCHOR: violation_report_logic]`, verified by `[@ANCHOR: test_tour_violation_report]`.

* **Home Routing:** Target view `/<slug>/home` `[@ANCHOR: controller_user_websites_home]`.

* **Site Creation:** `/<slug>/create_site` `[@ANCHOR: UX_CREATE_SITE]`, concurrency scaling proven by `[@ANCHOR: test_site_creation_performance_scaling]`.

* **Blog Routing:** `/<slug>/blog` `[@ANCHOR: controller_user_blog_index]`.

* **Blog Creation:** `/<slug>/create_blog` `[@ANCHOR: UX_CREATE_BLOG_POST]`.

* **Post Creation & Editing:** `POST /<slug>/create_blog_post` creates a blank "New Post" in the owner's blog (or redirects to `/<slug>/blog` if no blog exists yet) and redirects to `/blog_post/edit/<post_id>`, this module's own ownership-scoped title/body/publish form (the website-builder Edit toolbar needs the sitewide `website.group_website_designer` permission, which site owners do not hold).

* **Documentation:** Proxies knowledge records `/user-websites/documentation` `[@ANCHOR: controller_user_websites_documentation]`.

* **Appeals:** User submission endpoint `/website/submit_appeal` `[@ANCHOR: UX_SUBMIT_APPEAL]`.

* **Subscriptions:** `/<slug>/subscribe` `[@ANCHOR: UX_SUBSCRIBE]`. Unsubscribe verification with HMAC-SHA256 token validation over the model, record, partner and timestamp `[@ANCHOR: controller_unsubscribe_digest]`. The timestamp is signed but its age is not checked, so a valid link does not expire.

* **`GET /api/v1/user_websites/pending_reports`**: Returns a JSON object `{'count': int}` of unhandled violation reports. Restricted to administrators `[@ANCHOR: api_pending_reports]`; any other caller gets `{'count': 0, 'error': 'Forbidden'}` with HTTP 200, not an error status. Verified by `[@ANCHOR: test_admin_violation_toast_rpc]`.

* **GDPR Actions:** Privacy dashboard `/my/privacy` `[@ANCHOR: controller_my_privacy_dashboard]`. Data exports `/my/privacy/export` `[@ANCHOR: UX_GDPR_EXPORT]`. Data erasure via batched unlinks (5000 records per batch, run within the request under the `zero_sudo.gdpr_service_internal` service account) `/my/privacy/delete_content` `[@ANCHOR: UX_GDPR_ERASURE]`, then redirects to the public confirmation `/privacy/erased` `[@ANCHOR: user_websites:COMM_privacy_erased]`.

### 🚨 Privilege Deprecation & Cross-Module Execution (CRITICAL)
In adherence to the Micro-Service Account Pattern (ADR-0062), the `user_websites` internal service account (`user_websites.user_websites_service_account`) has been stripped of omnipotent ERP privileges. It **can no longer create or delete** core identity records (`res.users`, `res.partner`). Furthermore, it retains only microscopic access to framework tables (for example, read-only `1,0,0,0` on `res.company` and `res.partner.bank`, and read/write without create/delete `1,1,0,0` on `discuss.channel`; see `security/ir.model.access.csv` for the full list) strictly to satisfy Odoo's internal ORM cascade requirements (The Framework ACL Tax - ADR-0064). ADR-0062 and ADR-0064 are consolidated in `hams_shared/docs/adrs/MASTER_01_SECURITY_ZERO_SUDO.md`.

If your dependent module (e.g., `cloudflare`, `custom_dns`) needs to programmatically resolve slugs or provision websites, **you MUST NOT rely on the `user_websites` service account to bypass ACLs for you.** Instead, you must fetch your own domain-specific service account and pass it using the `override_svc_uid` parameter. Your module must explicitly declare the necessary Access Control Lists (`ir.model.access.csv`) for its own service account to perform the required operations.

### Programmatic Setup & Hooks
**The Secure Cached Resolver Pattern (ADR-0066, consolidated in `hams_shared/docs/adrs/MASTER_08_CORE_ARCHITECTURE_PERFORMANCE.md`)**: The `user_websites` module offers high-performance `@distributed_cache()` resolvers (a per-process cache backed by Redis, from `distributed_redis_cache`) for cross-module use. ALWAYS use these instead of `.search()` in frontend controllers to prevent database exhaustion. Callers **MUST** pass their own `override_svc_uid` to execute the database search under their own service account's context instead of relying on the default System Provisioner, preventing cross-module access rule failures due to the privilege deprecation mentioned above.
* **`res.users._get_user_id_by_slug(slug, override_svc_uid=None)`**: Resolves a user's slug to their User ID. Note: it resolves through raw SQL on the `user_websites_content_routing_view` SQL view, so `override_svc_uid` is accepted but currently has no effect.
* **`res.users.action_suspend_user_websites()`**: Forcefully unpublishes user content (moderation).
* **`res.users.action_pardon_user_websites()`**: Lifts the suspension and resets the strike count to 0 after an approved appeal. It does NOT republish content; previously unpublished pages and posts stay unpublished until manually restored.
* **`get_record_by_slug(slug)`** (from `edge.routing.mixin`, inherited by both `res.users` and `user.websites.group`): Resolves a user's or group's slug to its record ID (`@distributed_cache()`-cached). It takes no `override_svc_uid`; it runs under the `edge_routing` service account when that account exists. The controllers use it for both users and groups.
* **`website.page._get_page_id_by_url(url, website_id, override_svc_uid=None)`**: Resolves a page URL to its Page ID.
* **`RESERVED_SLUGS`**: A set of slugs that are reserved by the system and cannot be used by users or groups (e.g., `community`, `blog`, `my`). Available in `edge_routing.utils` (imported from there by this module).
* **`user_websites.owned.mixin`**: Inherit this in your custom models (e.g., `custom.portfolio`) to get the `owner_user_id`/`user_websites_group_id` ownership fields and the Proxy Ownership checks. The mixin does not override `create()`/`write()` itself: your model must call `self._check_proxy_ownership_create(vals_list)` from its `create()` and `self._check_proxy_ownership_write(vals)` from its `write()`, and must declare its own record rules (this module's `ir.rule` records are declared per model, not on the mixin).
  * **Mandatory Assignment:** Every record created by a non-administrator must be owned by a user or a group. If a non-public caller supplies neither `owner_user_id` nor `user_websites_group_id`, `owner_user_id` defaults to the caller; a public (not logged-in) caller who supplies neither gets an `AccessError`.
  * **Mutual Exclusivity:** A record CANNOT be owned by both a user and a group simultaneously. Attempting to assign both will raise a strict `ValidationError`.
* **Cache Invalidation Hooks:** Distributed slug caches are safely invalidated upon user mutation via `[@ANCHOR: slug_cache_invalidation]`, `[@ANCHOR: slug_cache_invalidation_unlink]`, `[@ANCHOR: group_slug_cache_invalidation]`, and `[@ANCHOR: group_slug_cache_invalidation_unlink]`.

* **String Utilities:** Safe slugification generation logic (`slugify()` in `edge_routing.utils`) `[@ANCHOR: utils_slugify]`.

* **Limits:** Individual page quota enforcement `[@ANCHOR: website_page_quota_check]`.

* **GDPR Hooks**: The module extends `_get_gdpr_export_data()` `[@ANCHOR: res_users_gdpr_export]`, tested by `[@ANCHOR: test_gdpr_export_hook]`, and `_execute_gdpr_erasure()` `[@ANCHOR: gdpr_sudo_erasure]`, tested by `[@ANCHOR: test_gdpr_erasure_pages]` and `[@ANCHOR: test_gdpr_erasure_posts]`. Dependent modules storing PII MUST override these to append their data to the export payload and hard-delete it during erasure.

* **Documentation Injection**: `knowledge` is a hard dependency of this module. Its `data/documentation.html` is declared in the manifest's `knowledge_docs` entry (with `"public": True`, so portal and public users can open it) and installed as a published `knowledge.article` by `zero_sudo`'s `ir.module.module._bootstrap_knowledge_docs()` when the registry loads; `/user-websites/documentation` redirects to that article `[@ANCHOR: documentation_bootstrap]`.
</public_api>

---

<stories_and_journeys>
## 4. Architectural Stories & Journeys

For detailed narratives and end-to-end workflows, refer to the following:

### Stories
* [Group Websites](user_websites/docs/stories/group_sites.md)
* [Content Moderation](user_websites/docs/stories/moderation.md)
* [Personal Site Management](user_websites/docs/stories/personal_site.md)
* [Privacy and GDPR Compliance](user_websites/docs/stories/privacy.md)
* [Technical Foundation and Utilities](user_websites/docs/stories/technical_foundation.md)

### Journeys
* [Extension and Customization](user_websites/docs/journeys/customization.md)
* [User Data Management (GDPR)](user_websites/docs/journeys/gdpr_compliance.md)
* [Content Reporting and Resolution](user_websites/docs/journeys/moderation_workflow.md)
* [First Time Site Setup](user_websites/docs/journeys/onboarding.md)
</stories_and_journeys>

---

<crons_and_subscriptions>
## 5. 📧 Weekly Digests & Subscriptions
* Features an automated `ir.cron` job (`send_weekly_digest` `[@ANCHOR: ir_cron_send_weekly_digest]`, `[@ANCHOR: send_weekly_digest]`) that iterates through `blog.post` objects and dispatches emails to followers. Re-entrant batching algorithm resumption is tested by `[@ANCHOR: test_cron_batching_resumption]`. QWeb Mail templates are verified by `[@ANCHOR: test_weekly_digest_mail_template]`.

* Utilizes HMAC-SHA256 tokens to generate secure, one-click `List-Unsubscribe` header links for GDPR/CAN-SPAM compliance `[@ANCHOR: test_weekly_digest_secret]`.

* **Background View Counter Sync:** High-throughput Redis view counters are safely flushed to Postgres via cron `[@ANCHOR: ir_cron_flush_view_counters]`, tested extensively by `[@ANCHOR: test_cron_redis_flush]`. Round-trips are optimized via a PostgreSQL procedure `[@ANCHOR: procedure_flush_view_counters]`.

* **High-Speed Simulation Tests:** The full operational load of the module is end-to-end verified via the High-Speed Simulation Environment `[@ANCHOR: simulation_environment]`.
</crons_and_subscriptions>

---

<function_reference>
## 6. Comprehensive Function Reference

This section ensures all module functions and their developer usage are thoroughly documented:

### `res.users` (`models/res_users.py`, `models/res_users_moderation.py`)
*   **`_async_unpublish_content(db_name, user_ids)`**: Background task to unpublish pages/posts for suspended users.
*   **`_register_hook()`**: System hook that creates the `sys_provisioner` service user (if missing) and maps the `user_websites.user_websites_service_account` XML ID to it, so other modules can resolve that account early. (Documentation is bootstrapped by `zero_sudo`, not here; see Documentation Injection above.)
*   **`_check_reserved_slugs()`**: ORM constraint ensuring `website_slug` doesn't conflict with system routes.
*   **`_is_admin()`**: Utility returning True if the user belongs to the `base.group_system` or `user_websites.group_user_websites_administrator`.
*   **`_get_page_limit()`**: Returns the user's own `website_page_limit` if set above 0, otherwise the configured global page limit (default 100).
*   **`_get_gdpr_streamed_keys()`**, **`_get_gdpr_export_data()`**, **`_execute_gdpr_erasure()`**: Privacy compliance methods for exporting and purging PII.
*   **`_compute_suspended_group_ids()`**: Computes related suspended groups for UI filtering.
*   **`action_suspend_user_websites()`** / **`action_pardon_user_websites()`**: State transitions for moderation.

### `user.websites.group` (`models/user_websites_groups.py`)
*   **`_async_unpublish_group_content(db_name, group_ids)`**: Background task to unpublish suspended group content.
*   **`_check_reserved_slugs()`**: Ensures group slugs don't overlap with system routes.
*   **`action_suspend_group_websites()`** / **`action_pardon_group_websites()`**: Moderation state transitions for groups.

### `website.page` (`models/website_page.py`)
*   **`_serve_page()`**: Override that increments the page's Redis view counter for non-admin visitors (a Redis failure is logged, never raised).
*   **`_invalidate_cloudflare_cache()`**: Purges Cloudflare edge caches upon page modification.
*   **`_sanitize_user_arch(arch_content)`**: XSS/SSTI sanitizer for user-submitted QWeb/HTML.
*   **`_trigger_malicious_arch_violation(vals, records=None)`**: Automatically reports users and issues strikes upon payload detection: one strike per distinct owner/group among `records` (keyed on that owner's first affected page URL). `write()` refuses a non-admin batch with a sanitizer-modified arch that spans more than one owner/group.
*   **`_flush_redis_view_counters()`**: Cron method flushing Redis counts to Postgres.
*   Standard ORM Overrides: **`create()`**, **`check_access()`**, **`write()`**, **`unlink()`** are heavily overridden to enforce `owner_user_id` proxies and quota limits.

### `blog.post` (`models/blog_post.py`)
*   **`_invalidate_cloudflare_cache()`**: Purges Cloudflare edge caches upon post modification.
*   **`_get_blog_urls()`**: Resolves slug paths for posts.
*   **`send_weekly_digest()`**: Compiles and dispatches the weekly digest email to subscribers.
*   Standard ORM Overrides: **`create()`**, **`check_access()`**, **`write()`**, **`unlink()`** enforce proxy ownership similar to `website.page`.

### `blog.blog` (`models/blog_blog.py`)
*   Standard ORM Overrides: **`create()`**, **`check_access()`**, **`write()`**, **`unlink()`** enforce proxy ownership: a non-admin may write or delete only a blog they own or that belongs to a group they are a member of; creation is limited to 5 blogs per user or group by default (`user_websites.global_blog_limit`).

### `user_websites.owned.mixin` (`models/user_websites_owned_mixin.py`)
*   **`_check_proxy_ownership_create(vals_list)`** / **`_check_proxy_ownership_write(vals)`**: Core assertions ensuring the operating user legally owns the modified proxy record.

### Moderation Models
`content.violation.report` itself (state machine, `action_mark_under_review()`, `action_dismiss()`, `action_take_action_and_strike()`, and the extensible `_apply_enforcement_action()` hook) moved to the `content_moderation` module on 2026-09-23 -- see that module's own README/model file. This module keeps only what's genuinely its own:
*   **`models/content_violation_report_moderation.py`**: this module's `content_group_id` extension field, its `_apply_enforcement_action()` override (the real strike-and-suspend consequence), `_increment_strike_count()`, and `_cron_notify_pending_reports()` (admin digest emails for pending reports -- kept here rather than genericized, since it's tied to this module's own `company_abuse_email` config parameter and service account).
*   **`models/content_violation_appeal.py`**: `_check_appeal_target()` validates that each appeal names exactly one target (a user or a group, not both); `action_approve()`/`action_reject()` are the admin workflow actions for appeals. Stayed in this module (not moved to `content_moderation`) since both actions call this module's own `action_pardon_user_websites()`/`action_pardon_group_websites()` directly, with no second consumer yet to justify a generic hook the way `content.violation.report` got one.

### Configuration (`models/res_config_settings.py`)
*   No `get_values()`/`set_values()` overrides: two plain `config_parameter` fields, `global_website_page_limit` (`user_websites.global_website_page_limit`, default 100) and `company_abuse_email` (`user_websites.company_abuse_email`). Administrator membership is managed on the group itself (the Settings page only links to it).

### SQL Views (`models/sql_views.py`)
*   **`init()`**: Executes the `CREATE OR REPLACE VIEW` statements for three plain (non-materialized) SQL views: the community directory (opted-in, active, non-suspended users with a slug), content routing (slug resolution), and the weekly digest. `UserWebsitesDbFunctions.init()` also creates the `increment_strike_count()` database function.

### Controllers (`controllers/main.py`, `controllers/user_websites_api.py`)
*   **`UserWebsitesController`**:
    *   `report_violation()`: POST handler for violation flags.
    *   `user_blog_index()`, `user_home_fallback()`: Resolvers for `/<slug>/blog` and `/<slug>/home`.
    *   `create_site()`, `create_blog()`: JIT provisioners for missing infrastructure.
    *   `create_blog_post()`, `blog_post_edit()`, `blog_post_edit_submit()`: New-post creation and the owner-only post edit form (`/blog_post/edit/<post_id>`).
    *   `documentation()`: Redirects to the module's published knowledge article.
    *   `community_directory()`: Renders `/community`.
    *   `privacy_dashboard()`, `privacy_export()`, `privacy_export_zip()`, `privacy_delete_content()`, `privacy_erased()`: GDPR endpoints (`privacy_export_zip()` mints a short-lived, single-use token and redirects to the separate GDPR export download service at `/api/v1/gdpr_export/download`).
    *   `submit_appeal()`: Handler for moderation appeals.
    *   `pending_reports()`: Plain HTTP GET endpoint returning JSON, used by the admin toast notifications.
    *   `subscribe()`, `unsubscribe()`: Handlers for weekly digest opt-in/opt-out.
*   **`UserWebsitesApi`** (`controllers/user_websites_api.py`):
    *   `api_domains()`: Public `GET /api/v1/user_websites/domains` returning `{"domains": [...]}`, every custom domain (`edge.routing.domain`) plus, if `ham_dns` is installed, every `ham.dns.zone` name, for Let's Encrypt certificate maintenance.

### System Hooks (`hooks.py`)
*   **`post_init_hook(env)`**: Triggered post-installation to execute initial setup routines: adds every existing real user (except the public user and service accounts) to `group_user_websites_user`, backfills empty `website_page.name` values from the linked view, creates partial indexes on published pages and posts, and flags the Cloudflare purge and User Websites service accounts as `is_service_account`.
</function_reference>
