# User Websites (`user_websites`)

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).*

Welcome to User Websites! This Odoo 19 module allows you to build and manage your own personal or group websites and blogs directly within Odoo. Whether you're a beginner or an expert, you'll find everything you need to create a professional online presence.

**Open Source Rule:** We built this for the open-source community. It runs perfectly on its own and does not rely on any proprietary code.

## 🌟 What Can I Do?

*   **Create Your Personal Site:** Get a unique URL (like `/yourname`) and use our easy drag-and-drop editor to build your pages.
*   **Start a Blog:** Share your thoughts and stories with the community. Everyone gets their own blog section.
*   **Collaborate with Groups:** Create shared websites for your team, club, or project.
*   **Join the Community:** Choose to show off your site in our public directory and discover what others are building.
*   **Control What Is Public:** You decide which of your pages are published. There is no expectation of privacy for anything you publish or enter. We also provide GDPR (the EU General Data Protection Regulation) data export and content-erasure tools, which exist because the law requires them.

## 🚀 Getting Started

### 1. Initialize Your Site
The first time you visit your personal URL (e.g., click on your name in the navbar), you'll see a "Create Your Site" button. Click it to set up your initial layout.

### 2. Customize Your Pages
Once initialized, use the **Edit** button in the top right corner of any of your pages. You can drag and drop different "snippets" (blocks of content like text, images, or contact forms) onto your page.

### 3. Start Blogging
Visit your blog page (e.g., `/yourname/blog`) and click "Create Your Blog" to start posting. You can manage all your posts from this central location.

## 🛡️ Community & Safety

We want to keep our community safe and professional.
*   **Report Violations:** If you see content that breaks our rules, every personal or group website page (home page, blog, or blog post) shows a "Report Violation" button to visitors other than its owner (on a group website, other than a group member). Our admins will review reports promptly.
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
* **Your Own Blog:** Each user or group gets its own Odoo `blog.blog` record (named `<Name> Blog`, public at `/<callsign>/blog`, its posts at `/<callsign>/blog/<post-slug>`), owned by that user or group, so only its owner (or the group's members) and administrators can change it. A user may have up to 5 blogs by default.
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
* **Ownership Validation:** Safely asserted by mixin create `mixin_proxy_ownership_create` and write `mixin_proxy_ownership_write` methods. Explicitly verified by `test_mixin_ownership_validation`.

* **Tenant Isolation:** Enforced via strict record rules verified by `test_tenant_view_isolation` and ACL (access control list) overhead elimination to prevent log spam `test_acl_overhead_loop_elimination`.
* **Lazy JIT Provisioning:** Websites and Blogs do not exist upon user creation. They are provisioned Just-In-Time when the owner visits their slug root and triggers a POST request to `create_site`. This ensures explicit user consent to publish.
</core_patterns>

---

<data_model>
## 2. 🗄️ Data Model Reference

### Extended `res.users`
* **`website_slug`**: URL-safe identifier. A slug is the first path segment of a user's (or group's) site URLs, e.g. `yourname` in `/yourname` and `/yourname/blog`; it must not be one of `RESERVED_SLUGS` (section 3).
* **`privacy_show_in_directory`**: Opt-in for the public `/community` directory.
* **`violation_strike_count`**: Number of upheld content violations.
* **`is_suspended_from_websites`**: If True, all personal content is forcefully unpublished (the unpublishing itself is done in the background by `action_suspend_user_websites()`; a direct write of this field only invalidates the slug/page resolver caches) and the user's slug routes return 404.
* **`appeal_ids`** (`One2many`): Links to Moderation Appeals.

### Content Models (`website.page`, `blog.post`)
* **`owner_user_id`**: The proxy owner.
* **`user_websites_group_id`**: For shared group websites.
* **`view_count`**: Privacy-friendly server-side view tracker (the server counts non-admin page views in Redis and a cron flushes them to this column every 15 minutes). Only `website.page` counts are incremented today; this module declares `blog.post.view_count` but never increments it.

### Moderation Models
* **`content.violation.report`** (model itself now in `content_moderation`, this module's own extension in `models/content_violation_report_moderation.py`): Stores abuse reports. Originator is masked from the target owner. The system automatically generates a report and issues a strike if a user attempts to inject malicious SSTI/XSS payloads into their site architecture `content_moderation:action_take_action_and_strike` (via this module's own enforcement-hook override `user_websites:COMM_apply_enforcement_action`), tested by `test_moderation_suspension`. Admin spam is prevented via a daily digest cron (`ir_cron_notify_pending_reports` `ir_cron_notify_pending_reports`, `cron_notify_pending_reports`, verified by `test_cron_pending_reports`) and a UI toast that is sticky but remembered in localStorage for 12 h per (count, newest id), with Review/Dismiss buttons, (`toast_notifications_logic`, `admin_toast_logic`, tested by `test_tour_toast_notifications`).

* **Security Auto-Moderation**: The `website.page` model includes `_sanitize_user_arch` `website_page_sanitize_arch`, verified by `test_website_page_sanitize_arch`, which forcefully strips dangerous QWeb directives (`t-*`) and JS event handlers.
* **Member CSS**: `<link>` is removed; `<style>` is rebuilt rule by rule and every `style="..."` attribute declaration by declaration with zero_sudo's `css_sanitizer` `user_websites:page_arch_style_attribute_filter`, verified by `test_user_arch_style_attribute_filter`: no `url()` except `#fragment`, no `image-set()`, `data:`, `expression`, escapes, and `position` only static, relative or absolute. A refused declaration is dropped (no strike); malformed CSS drops the whole attribute. Blog post content goes through `html_sanitize` instead and is not covered by this filter.
* **`content.violation.appeal`**: Used by suspended users to petition for account restoration.
</data_model>

---

<public_api>
## 3. 🐍 Public API & Extensibility Methods

### Explicit Dropzones
To prevent monolithic entanglement, `user_websites` provides the following explicitly designated dropzones. You MUST use `<xpath>` targeting these specific IDs and cite the corresponding Semantic Anchor:
* **Home Header:** `id="user_websites_dropzone_home_header"` -> `dropzone_home_header`

* **Home Footer:** `id="user_websites_dropzone_home_footer"` -> `dropzone_home_footer`

* **Global Navbar:** `id="user_websites_dropzone_navbar"` -> `dropzone_navbar`

* **Portal Templates:** `id="user_websites_dropzone_templates"` -> `dropzone_templates`

* **Snippets Sidebar:** `id="user_websites_dropzone_snippets"` -> `dropzone_snippets`

* **Website Layout:** `id="user_websites_dropzone_layout"` -> `dropzone_layout`

* **User Settings:** `id="user_websites_dropzone_users"` -> `dropzone_users`

* **Blog Post Form:** `id="user_websites_dropzone_blog_post"` -> `dropzone_blog_post`

* **Navbar Actions:** `id="user_websites_dropzone_navbar_actions"` -> `dropzone_navbar_actions`

* **Directory Card:** `id="user_websites_dropzone_directory_card"` -> `dropzone_directory_card`

* **Global Settings Form:** -> `dropzone_settings`

### Prohibited Dropzones
DO NOT USE user_websites_settings_dropzone. All settings views must now inherit directly from base.res_config_settings_view_form and target the //form element using the modernized `<app>`, `<block>`, and `<setting>` XML tags.

### Endpoints & Webhooks
* **Community Directory:** Renders public pages `/community` `UX_COMMUNITY_DIRECTORY`.

* **Violation Reporting:** Form endpoint `/website/report_violation` `user_websites:UX_REPORT_VIOLATION`, `violation_report_logic`, verified by `test_tour_violation_report`. Protected by the `form_shield` module (proof of work or question, honeypots, minimum time-to-submit, per-network, per-page and per-site limits `user_websites:report_limits`, `user_websites:report_violation_shield`, the form's fields `user_websites:report_violation_shield_fields`); the reported URL must be a page of this site (this host, the base URL or a website domain, http and https alike, www ignored `user_websites:report_own_site_hosts`) `user_websites:normalize_report_target` and a guest's report is refused when it is short, one random-looking string, link-heavy or lacks a valid address `user_websites:report_problem`. `GET /website/report_violation` is the question variant for browsers without scripts `user_websites:report_violation_page`. Helpers: the redirect path is forced to a single leading slash `user_websites:reported_path`, hosts compare without `www.` `user_websites:bare_host`, a guest's address may not start or end its local part with a dot `user_websites:local_part_ok`, the question page is rendered by `user_websites:render_report_page` and a refusal is answered by `user_websites:report_refusal`; the old per-address redirect-path anchor `user_websites:report_violation_redirect_target` stays.

* **Home Routing:** Target view `/<slug>` (the old `/<slug>/home` answers a permanent 301 to it, `user_websites:COMM_user_home_legacy_redirect`) `controller_user_websites_home`.

* **Site Creation:** `/<slug>/create_site` `UX_CREATE_SITE`, concurrency scaling proven by `test_site_creation_performance_scaling`.

* **Blog Routing:** `/<slug>/blog` `controller_user_blog_index`.

* **Blog Creation:** `/<slug>/create_blog` `UX_CREATE_BLOG_POST`.

* **Post Creation & Editing:** `POST /<slug>/create_blog_post` creates a blank "New Post" in the owner's blog (or redirects to `/<slug>/blog` if no blog exists yet) and redirects to `/blog_post/edit/<post_id>`, this module's own ownership-scoped title/body/publish form (the website-builder Edit toolbar needs the sitewide `website.group_website_designer` permission, which site owners do not hold).

* **Documentation:** Proxies knowledge records `/user-websites/documentation` `controller_user_websites_documentation`.

* **Appeals:** User submission endpoint `/website/submit_appeal` `UX_SUBMIT_APPEAL`.

* **Subscriptions:** `/<slug>/subscribe` `UX_SUBSCRIBE`. Unsubscribe verification with HMAC-SHA256 token validation over the model, record, partner and timestamp `controller_unsubscribe_digest`. The timestamp is signed but its age is not checked, so a valid link does not expire.

* **`GET /api/v1/user_websites/pending_reports`**: Returns a JSON object `{'count': int, 'latest_id': int, 'url': str}` of unhandled violation reports (`url` is the back-office pending queue, `/odoo/action-user_websites.action_content_violation_report_pending`). Restricted to administrators `api_pending_reports`; any other caller gets `{'count': 0, 'error': 'Forbidden'}` with HTTP 200, not an error status. Verified by `test_admin_violation_toast_rpc`.

* **GDPR Actions:** Privacy dashboard `/my/privacy` `controller_my_privacy_dashboard`. Data exports `/my/privacy/export` `UX_GDPR_EXPORT`. Data erasure via batched unlinks (5000 records per batch, run within the request under the `zero_sudo.gdpr_service_internal` service account) `/my/privacy/delete_content` `UX_GDPR_ERASURE`, then redirects to the public confirmation `/privacy/erased` `user_websites:COMM_privacy_erased`.

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
* **`RESERVED_SLUGS`**: A set of slugs that are reserved by the system and cannot be used by users or groups (e.g., `community`, `blog`, `my`). Available in `edge_routing.utils` (imported from there by this module). A slug is also refused when it is the first path segment of any route registered in the running database (`edge_routing.utils.routed_first_segments(env)`, read from the live routing map) or an installed language prefix; `edge_routing.utils.reserved_slug_words(env)` is the union the constraints use. The fixed set stays as the list a guard test (`test_09_reserved_slugs_cover_static_single_segment_routes`) keeps in step with the routes, and covers the words that are not routes (sub-paths of a member site, service names).
* **`user_websites.owned.mixin`**: Inherit this in your custom models (e.g., `custom.portfolio`) to get the `owner_user_id`/`user_websites_group_id` ownership fields and the Proxy Ownership checks. The mixin does not override `create()`/`write()` itself: your model must call `self._check_proxy_ownership_create(vals_list)` from its `create()` and `self._check_proxy_ownership_write(vals)` from its `write()`, and must declare its own record rules (this module's `ir.rule` records are declared per model, not on the mixin).
  * **Mandatory Assignment:** Every record created by a non-administrator must be owned by a user or a group. If a non-public caller supplies neither `owner_user_id` nor `user_websites_group_id`, `owner_user_id` defaults to the caller; a public (not logged-in) caller who supplies neither gets an `AccessError`.
  * **Mutual Exclusivity:** A record CANNOT be owned by both a user and a group simultaneously. Attempting to assign both will raise a strict `ValidationError`.
* **Cache Invalidation Hooks:** Distributed slug caches are safely invalidated upon user mutation via `slug_cache_invalidation`, `slug_cache_invalidation_unlink`, `group_slug_cache_invalidation`, and `group_slug_cache_invalidation_unlink`.

* **String Utilities:** Safe slugification generation logic (`slugify()` in `edge_routing.utils`) `utils_slugify`.

* **Limits:** Individual page quota enforcement `website_page_quota_check`.

* **GDPR Hooks**: The module extends `_get_gdpr_export_data()` `res_users_gdpr_export`, tested by `test_gdpr_export_hook`, and `_execute_gdpr_erasure()` `gdpr_sudo_erasure`, tested by `test_gdpr_erasure_pages` and `test_gdpr_erasure_posts`. Dependent modules storing PII MUST override these to append their data to the export payload and hard-delete it during erasure.

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
* Features an automated `ir.cron` job (`send_weekly_digest` `ir_cron_send_weekly_digest`, `send_weekly_digest`) that iterates through `blog.post` objects and dispatches emails to followers. Re-entrant batching algorithm resumption is tested by `test_cron_batching_resumption`. QWeb Mail templates are verified by `test_weekly_digest_mail_template`.

* Utilizes HMAC-SHA256 tokens to generate secure, one-click `List-Unsubscribe` header links for GDPR/CAN-SPAM compliance `test_weekly_digest_secret`.

* **Background View Counter Sync:** High-throughput Redis view counters are safely flushed to Postgres via cron `ir_cron_flush_view_counters`, tested extensively by `test_cron_redis_flush`. Round-trips are optimized via a PostgreSQL procedure `procedure_flush_view_counters`.

* **High-Speed Simulation Tests:** The full operational load of the module is end-to-end verified via the High-Speed Simulation Environment `simulation_environment`.
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
*   **`migrations/0.4/post-migration.py`**: removes the four `content.violation.report` ir.rule records and the `seq_content_violation_report` ir.sequence this module declared (in `noupdate="1"` blocks) before the extraction, which Odoo left behind on already-installed databases because it never deletes noupdate rows a module stops declaring. Only those exact `user_websites.*` xmlids, constrained to the expected model/code; content_moderation's own replacements are untouched `[@ANCHOR: user_websites_orphan_content_violation_cleanup]`.
*   **`models/content_violation_appeal.py`**: `_check_appeal_target()` validates appeals point to a valid suspension; `action_approve()`/`action_reject()` are the admin workflow actions for appeals. Stayed in this module (not moved to `content_moderation`) since both actions call this module's own `action_pardon_user_websites()`/`action_pardon_group_websites()` directly, with no second consumer yet to justify a generic hook the way `content.violation.report` got one.

### Configuration (`models/res_config_settings.py`)
*   No `get_values()`/`set_values()` overrides: two plain `config_parameter` fields, `global_website_page_limit` (`user_websites.global_website_page_limit`, default 100) and `company_abuse_email` (`user_websites.company_abuse_email`). Administrator membership is managed on the group itself (the Settings page only links to it).

### SQL Views (`models/sql_views.py`)
*   **`init()`**: Executes the `CREATE OR REPLACE VIEW` statements for three plain (non-materialized) SQL views: the community directory (opted-in, active, non-suspended users with a slug), content routing (slug resolution), and the weekly digest. `UserWebsitesDbFunctions.init()` also creates the `increment_strike_count()` database function.

### Controllers (`controllers/main.py`, `controllers/user_websites_api.py`)
*   **`UserWebsitesController`**:
    *   `report_violation()`: POST handler for violation flags.
    *   `user_blog_index()`, `user_home_fallback()`: Resolvers for `/<slug>/blog` and `/<slug>` (plus the `/<slug>/home` 301).
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

## Owner Interface (2026-10-12)

* **Owner toolbar** `user_websites:UX_OWNER_TOOLBAR` (`views/owner_site_templates.xml`, called from `user_navbar`): shown above the site's own navbar to the site's owner, or a member of the owning group, on every page of that site (an internal user sees it on their own site only). `?visitor=1` hides it.
* **Dashboard** `/my/site/<slug>` `user_websites:COMM_owner_site_dashboard`, `/my/site` redirects to the caller's own. Page editor `/my/site/<slug>/page/new|<id>` `user_websites:UX_OWNER_PAGE_EDITOR`, save `user_websites:COMM_owner_page_save`, publish toggles `user_websites:COMM_owner_page_publish` and `user_websites:COMM_owner_post_publish`, image upload `user_websites:COMM_owner_image_upload` (`controllers/owner_site.py`). Every route resolves the site from the database and answers 404 unless the caller owns it, or belongs to the owning group (`user_websites:owner_resolve_site`); an administrator who is not the owner gets the 404 too. The owner of a suspended or not-yet-eligible site gets a 403 page that says why instead (hidden from everyone else); a retired slug redirects to the live one.
* **Editor** `user_websites:UX_RICH_EDITOR`: a contenteditable WYSIWYG (`static/src/js/rich_editor.js`), no raw HTML and no external library. Odoo's own `html_editor` is an OWL backend component tied to the website builder's `website.group_website_designer` permission and its asset bundles; mounting it for portal members would hand them the sitewide editor, which this module deliberately never grants.
* **Sanitizer** `user_websites:member_html_sanitize` (`models/member_html.py`): strict allow-list in the request path for every caller, administrators included; hostile input is also filed through `website.page._trigger_malicious_arch_violation()`.
* **Limits**: title 120 characters, body 200,000 characters, images JPEG/PNG/GIF/WebP at most 2 MB and 100 per site, 30 writes and 10 uploads per user per minute (Redis counters, `user_websites:owner_rate_limit`; Redis down answers 503).
* **Retention**: no route erases. Unpublish keeps the record as a draft. Delete (`POST .../<page|post>/<id>/delete`, with a confirmation step) takes a page or post off the site and every public surface but archives it (`active` false, `deleted_by_owner` stamped; a page is moved to a private address so its own can be reused), and the dashboard's Deleted list can restore it, unpublished. The home page cannot be deleted. Uploaded images are public `ir.attachment` rows owned by the account (`res_model='res.users'`/`user.websites.group`, description `user_websites site image`); a member's account-erasure flow removes the images owned by that member's own account (not a group's images).
* **Layout** `user_websites:member_site_standalone_layout`: on a member site `layout_inherit_user_navbar` sets `no_header`/`no_footer`, so hams.com's header and footer are not rendered, and the site's own header (`user_navbar`) and footer (`member_site_footer`) are. Assets: `static/src/css/member_site.css` and `rich_editor.js` in `web.assets_frontend`.
* **Home address** `/<slug>` `user_websites:COMM_user_home_fallback`; `/<slug>/home` answers 301 `user_websites:COMM_user_home_legacy_redirect`.
* **Owner-site internals**: `/my/site` `user_websites:COMM_owner_site_entry`; the new-page and edit-page forms `user_websites:COMM_owner_page_new`, `user_websites:COMM_owner_page_edit`; a page's arch is the fixed member-page frame around sanitized body markup `user_websites:owner_page_arch` and the editor reads the body back out of it `user_websites:owner_page_arch_body`; uploads are accepted by magic number, not by file name or declared type `user_websites:owner_image_mimetype`.

### `UserWebsitesOwnerController._rate_limit(bucket, limit)` [@ANCHOR: user_websites:owner_rate_limit]

Counts one write or upload by the signed-in owner against a per-minute allowance; called first by every owner route that changes something.

| Argument | Type and range | Meaning |
|---|---|---|
| `bucket` | `str`: `"write"` or `"upload"` | Which allowance. The Redis key is `uw:rl:<database>:<bucket>:<user id>`, so each user has one counter per bucket per database. |
| `limit` | `int`, 1 or more | The most hits allowed in the window: `WRITES_PER_MINUTE` (30) for `"write"`, `UPLOADS_PER_MINUTE` (10) for `"upload"`. |

| Outcome | Meaning |
|---|---|
| returns `None` | This hit is within `limit`. |
| `TooManyRequests` (HTTP 429) | The count including this hit is above `limit`: "Too many changes in a minute." |
| `ServiceUnavailable` (HTTP 503) | Redis raised `RedisError`. The limiter fails closed, because the write paths do real work (HTML sanitising, image decoding) that nothing else bounds. The error is logged. |

Behaviour: the counter is `distributed_redis_cache.expiring_counter(client, key, 60)`, so the 60-second window starts at the first hit and is never extended, and a counter can never be left without its expiry (an `EXPIRE` that failed after the increment used to lock the owner out for ever; a key an older build left without one heals on its next use). Every call counts, refused hits included, so an owner who keeps hammering stays refused until a quiet minute. One Redis round trip, no locks or threads. Tests: `test_15_write_rate_limit`, `test_15b_the_write_rate_limit_counter_always_has_an_expiry`.
### Citation lists for the owner interface

* **Editor functions** (`static/src/js/rich_editor.js`, exercised by `test_tour_owner_site`): `user_websites:rich_editor_apply_link`, `user_websites:rich_editor_clean_node`, `user_websites:rich_editor_close_link_bar`, `user_websites:rich_editor_image_picked`, `user_websites:rich_editor_link_bar_click`, `user_websites:rich_editor_link_keydown`, `user_websites:rich_editor_open_link_bar`, `user_websites:rich_editor_paste`, `user_websites:rich_editor_remember_range`, `user_websites:rich_editor_restore_range`, `user_websites:rich_editor_say`, `user_websites:rich_editor_start`, `user_websites:rich_editor_sync_field`, `user_websites:rich_editor_toolbar_click`, `user_websites:rich_editor_toolbar_state`, `user_websites:rich_editor_wrap_code`.
* **Helper functions** (owner interface and sanitizer, each exercised by `tests/test_owner_site.py`): `user_websites:fn_main_serve_core_fallback`, `user_websites:fn_owner_escape`, `user_websites:fn_owner_escape_attr`, `user_websites:fn_owner_svc`, `user_websites:fn_owner_owner_domain`, `user_websites:fn_owner_qcontext`, `user_websites:fn_owner_report_if_hostile`, `user_websites:fn_owner_clean_body`, `user_websites:fn_owner_own_page`, `user_websites:fn_owner_back_to_editor`, `user_websites:fn_owner_attachment_owner`, `user_websites:fn_hmtl_clean_url`, `user_websites:fn_hmtl_is_hostile`, `user_websites:fn_hmtl_sanitize_member_html`, `user_websites:fn_hmtl_clean_children`, `user_websites:fn_hmtl_clean_attributes`, `user_websites:fn_hmtl_drop`, `user_websites:fn_hmtl_unwrap`, `user_websites:fn_hmtl_menu_pages`, `user_websites:fn_users_user_websites_menu_pages`, `user_websites:fn_group_user_websites_menu_pages`.
* **SVG schematics** survive the sanitizer through zero_sudo's SVG allow-list `user_websites:member_html_svg_allowlist`; every other active or unknown element is removed.

* `user_websites:COMM_owner_ascii_digits` `_is_ascii_digits(value)` (owner site controller) is True only for a non-empty string of ASCII digits, the strings `int()` accepts; a page id such as the superscript two answers 404, not 500. **Tests:** `test_10b_a_page_id_that_is_not_ascii_digits_is_a_404_not_a_500`.

### Owner-site controller and page sanitizer internals

* `[@ANCHOR: user_websites:fn_site_response_init]` `SiteResponse(response)` (`controllers/owner_site.py`) is the exception `_resolve_site()` raises to end an owner-site request with a response of its own. **Purpose:** carry a redirect from a retired slug, or the "what is missing" page for an ineligible member, up to `answers_site_response`. **Arguments:** `response`, a werkzeug response built by the caller (`request.redirect(...)` or a rendered 403 page); it is kept unchanged as the `.response` attribute. **Returns / errors:** a constructor; it raises nothing. **Concerns:** it is deliberately not an `HTTPException`, because Odoo's frontend error handler would replace such an exception with its own error page (a 301 would become a 418); no locks or I/O. **Tests:** `test_03_owner_is_told_what_is_missing_and_cannot_create`.
* `[@ANCHOR: user_websites:fn_owner_own_page_any]` `_own_page_any(profile_user, profile_group, page_id)` finds one `website.page` the signed-in owner owns, including one the owner deleted (archived, its address parked). **Arguments:** `profile_user` (a `res.users` record or `None`) and `profile_group` (a `user.websites.group` record or `None`), the owner of the site being edited, exactly one of which is set; `page_id` (`int`), the page's id. **Returns:** the page record. **Errors:** `request.not_found()` (HTTP 404) when the id does not exist or the page belongs to a different user or group, so another member's page id looks the same as a missing one. Unlike `_own_page` it does not check that the page's URL is under the owner's slug, and it searches with `active_test=False`; only the restore route uses it. It runs as the user-websites service account. **Tests:** `test_05_restore_brings_it_back_unpublished`.
* `[@ANCHOR: user_websites:fn_owner_back]` `_back(site_slug, flag)` ends a dashboard action by redirecting to the owner's dashboard. **Arguments:** `site_slug` (`str`, the site's slug, URL-quoted with nothing safe so a slug cannot add path segments) and `flag` (`str`, the query string that tells the dashboard what to say, for example `saved=deleted` or `error=home_delete`). **Returns:** a 303 (see other) response to `/my/site/<slug>?<flag>`, so a reload does not repeat the POST. No errors, no side effects. **Tests:** `test_05_restore_brings_it_back_unpublished`, `test_07_home_page_cannot_be_deleted`.
* `[@ANCHOR: user_websites:fn_page_remove_keeping_tail]` `_remove_keeping_tail(element)` (`models/website_page.py`) removes an lxml element from its parent without losing the text after it. **Argument:** `element`, an lxml element; the sanitizer calls it for every script, iframe, object, embed, base, link, undeclared-namespace or emptied style element it strips. **Behaviour:** the element's tail text moves to the previous sibling's tail, or to the parent's text when there is no previous sibling, then the element is removed with its children. An element with no parent is left alone. Returns `None`; it changes the tree in place. **Tests:** `test_01_link_is_removed_whatever_its_rel_and_text_is_kept`.
* `[@ANCHOR: user_websites:fn_page_malicious_arch_violation_targets]` `_malicious_arch_violation_targets(vals, records)` decides which accounts are struck when the sanitizer strips an attack from a page. **Arguments:** `vals` (`dict`, the values being written; `owner_user_id`, `user_websites_group_id` and `url` are read) and `records` (a `website.page` recordset or empty/`None`, the pages being written). **Returns:** a list of `(owner_id, group_id, url)` tuples, one per account to strike. Ownership named in `vals`, or no `records`, gives one target (URL from `vals`, else the first record's). Otherwise the records are grouped by their distinct (owner, group), each gets one target keyed on its first page's URL, so a batch over many pages of one owner costs one strike. No errors, no database writes. **Tests:** `test_03_two_owner_helper_strikes_each_owner_once`.
* `[@ANCHOR: user_websites:fn_page_file_malicious_arch_report]` `_file_malicious_arch_report(svc_uid, owner_id, group_id, target_url, reporter_id)` records the automated violation for one target and strikes it. **Arguments:** `svc_uid` (`int`, the user-websites service account's user id, who creates and acts on the report); `owner_id` and `group_id` (`int` or `False`, the account to strike); `target_url` (`str`, the page address); `reporter_id` (`int`, the user who made the write). **Behaviour:** if a `content.violation.report` for the same URL and reporter already exists it takes action on it again (another strike); otherwise it creates an automated-security-alert report and takes action on that. Returns `None`; writes reports and strikes in the caller's transaction. Called once per target by `_trigger_malicious_arch_violation`. **Tests:** `test_03_two_owner_helper_strikes_each_owner_once`.

### Test-package pinning helpers (`tests/open_source_defaults.py`)

These make this module's own tests behave the same whatever hams.com modules share the database; they are test code and ship no behaviour. See the module docstring for why.

* `[@ANCHOR: user_websites:fn_open_source_set_committed_param]` `_set_committed_param(key, value)` upserts an `ir_config_parameter` row on its own committed connection (so real HTTP worker threads see it) and returns the old value. **Arguments:** `key` (`str`), `value` (`str`, or `None` to delete the row). **Returns:** the previous value, or `None` if there was none. **Concerns:** it opens its own cursor and commits; callers register it as a class cleanup to put the old value back. **Tests:** `test_01_set_committed_param_upserts_deletes_and_returns_the_old_value`.
* `[@ANCHOR: user_websites:fn_open_source_eligibility_gaps]` `_open_source_eligibility_gaps(self)` is the open-source `_website_eligibility_gaps()` (an empty list), called on the class that defines it so no hams.com override applies. It is installed over the registry's `res.users` method for the life of each test class. **Argument:** `self`, a single `res.users` record. **Tests:** `test_02_the_open_source_gaps_are_empty_whatever_a_deployment_overrides`.
* `[@ANCHOR: user_websites:fn_open_source_wrap]` `_wrap(cls)` replaces `cls.setUpClass` so that, for the life of the class, the slug policy is `name_callsign`, the form-shield tuning parameters are removed, and `_website_eligibility_gaps` is the open-source one; class cleanups restore the old values. **Argument:** `cls`, a database test case class (its own `setUpClass`, or the inherited one, runs in the middle). Returns `None`. **Tests:** `test_03_wrap_pins_the_policy_and_the_gaps_for_the_class_and_restores_them`.
* `[@ANCHOR: user_websites:fn_open_source_pin_package]` `pin_package(package_name)` wraps every `TransactionCase` or `HttpCase` class defined in a loaded module named `<package_name>.test_*`, once per class (it sets `_open_source_pinned`). **Argument:** `package_name` (`str`, the tests package, `__name__` of `tests/__init__.py`); it must be called after the test modules are imported. Returns `None`. **Tests:** `test_04_pin_package_wraps_the_database_test_cases_of_the_named_package_only`.

---

## 7. Anchor Reference (ADR 0113)

Each entry below documents one anchor completely: purpose, signature, arguments, results, errors, side effects and the tests that cite it. Entries moved here from the one-line mentions in sections 1 to 6, which now name the anchor in backticks only.

### `user_websites.owned.mixin._check_proxy_ownership_create(vals_list)` [@ANCHOR: mixin_proxy_ownership_create]

**Purpose:** the create-time half of the Proxy Ownership Pattern. A model inheriting `user_websites.owned.mixin` (`website.page`, `blog.post`, `blog.blog`, your own) calls it from its own `create()` before the write. **Argument:** `vals_list`, a list of value dicts, one per record about to be created; it is changed in place, because a missing owner is filled in. **Returns:** `None`. **Rules:** a privileged caller (superuser, `base.group_system`, the User Websites administrator group, or the capability-only `group_user_websites_ownership_bypass` the provisioning account carries) may assign any owner. Everyone else must own the record: with neither owner field set, a logged-in caller becomes `owner_user_id`; a public caller gets `AccessError`. **Errors:** `ValidationError` when both `owner_user_id` and `user_websites_group_id` are set (checked for administrators too); `AccessError` when the owner is another user, or the group does not exist or the caller is not one of its `member_ids`. **Side effects:** group members are read once for the whole batch, as the service account, so no query runs per record; when the service account is not provisioned yet the group check is skipped and a debug line is logged. **Tests:** `test_mixin_ownership_validation`, `test_api_armor_mandatory_assignment`, `test_ownership_bypass_group_gates_mixin_create`.

### `user_websites.owned.mixin._check_proxy_ownership_write(vals)` [@ANCHOR: mixin_proxy_ownership_write]

**Purpose:** the write-time half of the pattern: owners may edit their record but may not hand it to someone else. Call it from your model's `write()` on the recordset being written. **Argument:** `vals`, the dict about to be written. **Returns:** `None`. **Rules:** a privileged caller (same four tests as the create check) may change ownership, but the result is still checked per record: after the write a record must not end up owned by both a user and a group, using the existing owner for any field `vals` does not mention. A non-privileged caller who puts `owner_user_id` or `user_websites_group_id` in `vals` at all, even with the current value, gets `AccessError`. **Errors:** `ValidationError` for dual ownership, `AccessError` for a transfer attempt. **Side effects:** owner and group are pre-fetched with `mapped()` so the loop does not query per record; nothing is written. **Tests:** `test_mixin_ownership_validation`, `test_api_armor_mutual_exclusion`, `test_ownership_bypass_group_gates_mixin_write`.

### `test_mixin_ownership_validation` [@ANCHOR: test_mixin_ownership_validation]

Test `test_03_mixin_ownership_validation` in `tests/test_sdk_extensibility.py`. It creates a portal member in the User Websites user group, then, as that member, tries to create a `website.page` whose `owner_user_id` is another user. The test passes only when `AccessError` is raised, proving that the mixin blocks cross-user creation and that a dependent module cannot assign pages to someone else. It cites both `mixin_proxy_ownership_create` and `mixin_proxy_ownership_write`, and is also cited from the mixin source. It runs inside one database transaction, commits no data of its own beyond the fixture, and makes no network or Redis calls.

### `test_tenant_view_isolation` [@ANCHOR: test_tenant_view_isolation]

Test `test_03_tenant_view_isolation` in `tests/test_performance_regressions.py`, cited from `website.page.write()`. **Given** two members whose home pages were created by the service account, each with its own `t-name` key and arch, **when** member A writes new arch (a `<div>` holding a marker string) to A's page, **then** a guest fetching A's page gets HTTP 200 and sees the marker, while a guest fetching B's page gets HTTP 200 without it. This guards the detached-view design: each page owns its own `ir.ui.view`, so editing one never changes a template shared with other tenants. It also passes through `check_access("write")` and `_check_proxy_ownership_write`. It needs the HTTP test server and runs in one transaction.

### `test_acl_overhead_loop_elimination` [@ANCHOR: test_acl_overhead_loop_elimination]

Test `test_02_acl_overhead_loop_elimination` in `tests/test_performance_regressions.py`, cited from `website.page.check_access()`. **Given** several members' published home pages, **when** an unauthenticated guest requests `/` and `/community` while the test captures the `odoo.addons.base.models.ir_model` logger, **then** no WARNING containing "Access Denied by ACLs" that names `ir.ui.view` or `website` may appear (the test fails with the count if one does). The code under test is the early owner or group-member check in `check_access` for `write` and `unlink`: it raises one cheap `AccessError` instead of letting the `ir.rule` engine log an INFO or WARNING line per page, which is log spam that grows with the number of tenants. No locks; it uses the HTTP test server.

### `website.page._sanitize_user_arch(arch_content)` [@ANCHOR: website_page_sanitize_arch]

**Purpose:** makes member-written page markup safe to store and render. **Argument:** `arch_content`, a string of QWeb or HTML arch (wrapped in a synthetic `<root>` and parsed with lxml `recover=True`). **Returns:** the tuple `(sanitized_string, was_modified)`; an empty input returns `(input, False)`. **What it does, in order:** rebuilds every `<svg>` from zero_sudo's allow-list; removes elements with undeclared namespace prefixes; strips `script`, `iframe`, `object`, `embed` and `base` (any letter case); removes `<link>`; rebuilds `<style>` text through zero_sudo's stylesheet filter; filters every `style` attribute; neutralises `<meta http-equiv="refresh">`; blocks `javascript:`, `data:` and `vbscript:` URLs in `href`, `src`, `content`, `formaction`, `action`, `poster` and `background`; keeps only `t-name` (safe characters) and `t-call` naming a fixed list of four templates, moving anything else, including `on*` handlers and `xmlns`, to a `data-blocked-*` attribute; and escapes `<` in text nodes. **Strikes:** `was_modified` is True only for attack-shaped changes (script, SVG, QWeb, URL, refresh), not for CSS filtering. **Errors:** an internal `KeyError` or `ValueError` returns `"<div>Sanitization Error</div>"` with True. It is idempotent, does no I/O, takes no locks, and is called from `create()` and `write()` for non-administrators. **Tests:** `test_website_page_sanitize_arch`, `test_website_page_sanitize_arch_bare_env_bypass`.

### `test_website_page_sanitize_arch` [@ANCHOR: test_website_page_sanitize_arch]

Test `test_07_qweb_arch_sanitization` in `tests/test_orm_security.py`. A non-administrator member creates a page whose arch holds a `<script>`, an `<iframe src="http://evil.com">` and a `div` with a `t-eval` Python expression and an `onmouseover` handler. The test asserts that the stored arch no longer contains the script tag or the iframe, that `t-eval` and `onmouseover` were moved to `data-blocked-t-eval` and `data-blocked-onmouseover`, and that an automated `content.violation.report` now exists against the member as `content_owner_id` (so the strike path ran). It then repeats the attack through `write()`. It also cites `COMM_trigger_malicious_arch_violation`. One transaction, no network.

### Style attributes in `_sanitize_user_arch` [@ANCHOR: user_websites:page_arch_style_attribute_filter]

**Purpose:** a member's `style="..."` attribute is a CSS exfiltration and overlay vector (`background:url(https://evil/..)`, `position:fixed` covering the page), so every element's `style` is filtered. **Input and output:** each attribute value goes to zero_sudo's `sanitize_style_attribute(value)`, which returns `(cleaned_value, dropped)`. The attribute is deleted and set again only if the cleaned value is not empty; the attribute name is matched in any letter case, as the browser would. **Rules enforced by that filter:** no `url()` except a `#fragment`, no `image-set()`, `data:`, `expression` or escape sequences, and `position` only `static`, `relative` or `absolute`. A refused declaration is dropped one by one; malformed CSS drops the whole attribute. **No strike:** removal is logged at info level and `was_modified` is not set, since a fixed header is not an attack. SVG subtrees were already rebuilt and keep only their own filtered style. Blog posts use `html_sanitize` instead. **Tests:** `test_user_arch_style_attribute_filter`.

### `test_user_arch_style_attribute_filter` [@ANCHOR: test_user_arch_style_attribute_filter]

The class `TestUserArchStyleAttribute` in `tests/test_user_arch_style_link.py` (tagged `post_install`) calls `website.page._sanitize_user_arch` directly. Its five tests cover: every hostile value in the `STYLE_ATTR_HOSTILE` table is removed without a strike (`modified` False) and leaves no `evil.example`, `fixed` or `expression` text; a harmless `color:red;margin:0 auto` survives while the hostile declaration beside it goes, including an upper-case `STYLE`; a malformed attribute is dropped whole and empty or blank ones removed; every element and nesting level, including `t` templates, is covered; and a `style` inside an `<svg>` still uses the SVG filter. No database writes.

### `website.page.create` page quota check [@ANCHOR: website_page_quota_check]

**Purpose:** enforces the per-owner page limit at creation, so a member cannot build an unbounded number of pages. **Where:** step 2 of `website.page.create(vals_list)` in `models/website_page.py`; `blog.blog` and `blog.post` create methods cite it for their own quotas. **Rules:** for each distinct `owner_user_id` in the batch, the limit is the user's `_get_page_limit()` (their own `website_page_limit` if above 0, otherwise `user_websites.global_website_page_limit`, default 100). Existing pages come from one `_read_group` as the service account, plus the batch's own count per owner. For group-owned pages the limit is the global one, counted per `user_websites_group_id`. **Errors:** `ValidationError("You have reached your limit of N website pages.")` when existing plus new exceeds the limit. **Fallbacks:** if the service account lookup raises `AccessError` during early installation, the code reads `sys_provisioner` by SQL, else uid 1. Read-only apart from the raise. **Tests:** `test_page_quota_limit`, `test_page_limits` in `tests/test_page_limits.py` (user, global fallback and group cases).

### `test_site_creation_performance_scaling` [@ANCHOR: test_site_creation_performance_scaling]

Test `test_01_site_creation_query_scaling` in `tests/test_performance_regressions.py`, cited from `website.page.create()`. **Given** a list of test users, **when** the service account creates each user's home page in turn (a `t-name` arch calling `user_websites.template_default_home`), after flushing and reading `cr.sql_log_count` before and after, **then** the number of SQL queries per creation must stay constant (O(1)) from the first user to the last. It guards against the older N+1 view invalidation storm, where each new page re-validated every other tenant's view. A growing count fails the test. No Redis or network access.

### `slug_cache_invalidation` [@ANCHOR: slug_cache_invalidation]

**What is anchored:** the rule that changing a user's `website_slug` (or `name`, which can derive one) must make the next `res.users.get_record_by_slug(slug)` re-read the database. **Code:** the hook is `write()` of `edge.routing.mixin` (`edge_routing/models/routing_mixin.py`), which `res.users` inherits. It collects the old and new slugs, writes, then calls `zero_sudo.security.utils._notify_cache_invalidation(model, slugs)` (a Postgres `NOTIFY` that clears the `@distributed_cache` entries in every worker). **Errors:** reserved slugs raise `ValidationError`; a failing notify is logged and swallowed, so the record update still succeeds. **Test:** `test_04_bdd_ormcache_query_counting_slugs` in `tests/test_audit_edge_cases.py` primes the cache, asserts a repeat lookup runs no `res_users` SQL, changes the slug, and asserts the new slug resolves.

### `slug_cache_invalidation_unlink` [@ANCHOR: slug_cache_invalidation_unlink]

**What is anchored:** deleting a user must also drop that user's slug from the distributed cache, so `/<slug>` stops resolving to a deleted account. **Code:** `unlink()` of `edge.routing.mixin` reads the non-empty `website_slug` values first, calls `super().unlink()`, then `_notify_cache_invalidation(self._name, slugs)`. A notify failure (a `pg_notify` database error) is logged as a warning and never blocks the delete. **Returns:** the result of the base unlink. **Test:** the last line of `test_04_bdd_ormcache_query_counting_slugs` in `tests/test_audit_edge_cases.py` unlinks the cache user after the lookup checks; the same test also covers `slug_cache_invalidation`. Unknown: no test yet asserts that the cache is empty after the unlink; to-do.

### `group_slug_cache_invalidation` [@ANCHOR: group_slug_cache_invalidation]

Same rule as `slug_cache_invalidation`, for the model `user.websites.group`: a group's slug change must invalidate the cached `get_record_by_slug`. **Code:** `user.websites.group` inherits `edge.routing.mixin`, so `write()` there computes old and new slugs and calls `_notify_cache_invalidation("user.websites.group", slugs)`, while the same reserved-slug check applies. **Test:** `test_05_bdd_ormcache_query_counting_group_slugs` in `tests/test_audit_edge_cases.py` creates a group, primes the cache, checks that the cached lookup executes no `user_websites_group` SQL (a temporary wrapper on `cr.execute`), renames the slug, and checks the new slug resolves. The cache is per model name, so users and groups never share an entry.

### `group_slug_cache_invalidation_unlink` [@ANCHOR: group_slug_cache_invalidation_unlink]

Deleting a `user.websites.group` clears its slug from the distributed cache through the `unlink()` of `edge.routing.mixin`: it reads the slugs first, unlinks, then sends `_notify_cache_invalidation("user.websites.group", slugs)`. A failed notification is logged as a warning and the delete still commits. **Test:** `test_05_bdd_ormcache_query_counting_group_slugs` in `tests/test_audit_edge_cases.py` ends by unlinking the group it created; like the user case, it proves the delete works but does not yet assert the cache entry is gone (to-do). Also see `group_slug_cache_invalidation`.

### `slugify(s, max_length=None)` in `edge_routing.utils` [@ANCHOR: edge_routing:utils_slugify]

**Purpose:** the one slug generator for user and group site addresses (the first path segment such as `yourname`). It is a local copy so the module does not depend on Odoo's `http_routing` slugify, which moves between versions. **Arguments:** `s` (any value; falsy gives `""`), `max_length` (optional int; the slug is cut to that length and trailing `-` removed, and `0` really means an empty slug). **Returns:** `str` made of `a-z`, `0-9` and single hyphens: the text is stripped and lower-cased, accents are folded to ASCII with NFKD (characters with no ASCII form are dropped), every other run of characters becomes one `-`, and leading and trailing hyphens are removed. No errors, no I/O. It does not check reserved words; `RESERVED_SLUGS` and `reserved_slug_words(env)` do that. The code anchor is `edge_routing:utils_slugify`; test `test_utils_slugify`.

### `res.users._get_gdpr_export_data()` [@ANCHOR: res_users_gdpr_export]

**Purpose:** this module's addition to the privacy export of one user (GDPR access request). **Signature:** no arguments besides `self`, which must be one record (`ensure_one`). **Returns:** the dict from the parent chain (compliance and other modules), with the `user` entry extended by `name`, `email` and `website_slug` (False when unset). Pages, blog posts, reports and appeals are not put in this dict: they are streamed by `_get_gdpr_streamed_keys()`, which adds the generators `pages`, `blog_posts`, `submitted_reports` and `appeals` (read in batches of 1000 by id) so a large account cannot exhaust memory. **Errors:** none of its own. Read-only, no locking. Dependent modules storing personal data override the hook and add their own keys. **Tests:** `test_gdpr_export_hook`.

### `test_gdpr_export_hook` [@ANCHOR: test_gdpr_export_hook]

Test `test_01_gdpr_export_hook` in `tests/test_sdk_extensibility.py`. It creates a `website.page` owned by the test user and commits, then calls `_get_gdpr_export_data()` and `_get_gdpr_streamed_keys()` on that user. It asserts that the data has a `user` key and that `pages` appears among the streamed keys, so large content lists are streamed rather than loaded into the dict (the message names the out-of-memory risk). It is cited from the hook in `models/res_users.py` and tests `res_users_gdpr_export`. A failure means the hook or the stream key was removed.

### `res.users._execute_gdpr_erasure()` [@ANCHOR: gdpr_sudo_erasure]

**Purpose:** hard-deletes everything this module holds for one user when they exercise the right to erasure. **Signature:** `self` is one `res.users` record; no other arguments; returns `None`. **Steps:** as the `zero_sudo.gdpr_service_internal` service account (so the search finds the target's rows whoever the caller is, not only the caller's own), it loops over `website.page`, `blog.post` and `blog.blog` owned by the user, 5000 at a time, unlinking each batch inside a savepoint; then removes up to 500 site-editor images (`ir.attachment` with description `user_websites site image` on the user); then sets `privacy_show_in_directory` False. **Errors and locking:** a concurrent-update, serialization or deadlock error is retried after 0.5 s; any other error is raised. **Side effects:** commits after each batch outside test mode and pauses 0.1 s between full batches unless `ODOO_DISABLE_SLEEPS` is set. Group-owned content is not deleted. **Tests:** `test_gdpr_erasure_pages`, `test_gdpr_erasure_posts`.

### `test_gdpr_erasure_pages` [@ANCHOR: test_gdpr_erasure_pages]

Test `test_02_right_to_erasure` in `tests/test_privacy_gdpr.py` (a page half of it; the source comment in `res_users.py` cites this anchor). An authenticated member with a page and a post POSTs to `/my/privacy/delete_content` with a CSRF (cross-site request forgery) token. The test asserts HTTP 200 landing on `/privacy/erased` with the text "Your data has been erased", the account deactivated, the page record gone (`exists()` False) and the directory flag off. It also cites `gdpr_sudo_erasure` and `UX_GDPR_ERASURE`. Another test, `test_01_gdpr_erasure_suspended_user` in `tests/test_audit_edge_cases.py`, checks that a suspended user's content is unlinked, not merely unpublished.

### `test_gdpr_erasure_posts` [@ANCHOR: test_gdpr_erasure_posts]

The blog half of `test_02_right_to_erasure` in `tests/test_privacy_gdpr.py`: after the same POST to `/my/privacy/delete_content` the test asserts that the member's `blog.post` no longer exists, with the message that the blog posts must be permanently deleted. It tests the second loop of `_execute_gdpr_erasure()`, which unlinks `blog.post` rows owned by the user in batches of 5000 as the GDPR service account. A failure would mean posts stay visible after an erasure request, a legal compliance defect. See `gdpr_sudo_erasure`.

### `content.violation.report.action_take_action_and_strike()` [@ANCHOR: content_moderation:action_take_action_and_strike]

Defined in the `content_moderation` module (its own README documents the state machine); this module only extends its hook. **Behaviour as used here:** an administrator presses "Take Action and Strike" on a report; every record in `self` that is not already `action_taken` or `dismissed` is moved to `action_taken`, then `_apply_enforcement_action()` runs once. Reports in a terminal state are skipped, so saving the same page again cannot add a second strike for one report. User Websites' override of the hook adds the real strike and suspension (`COMM_apply_enforcement_action`). The method is exposed to the web client through `@public_rpc`; who may call it is decided by the module's access rules. **Test:** `test_moderation_suspension`.

### `content.violation.report._apply_enforcement_action()` [@ANCHOR: user_websites:COMM_apply_enforcement_action]

**Purpose:** the consequence of an upheld report. Override of `content_moderation`'s hook, in `models/content_violation_report_moderation.py`. **Signature:** `self` is one report (`ensure_one`); returns `None`. **Behaviour:** with `content_owner_id` set, it calls the stored procedure `increment_strike_count('res_users', id)` (a row lock plus increment in SQL, through `_increment_strike_count`), sends model invalidation, re-reads `violation_strike_count`, and at 3 or more strikes calls `action_suspend_user_websites()` if the user is not yet suspended. With `content_group_id` it does the same for `user_websites_group` and `action_suspend_group_websites()`. With neither, it falls back to the base hook (a chatter note, no strike). In every strike case a note with the new count is posted to the report as the mail service account. **Errors:** database errors propagate. **Tests:** `test_moderation_suspension`.

### `test_moderation_suspension` [@ANCHOR: test_moderation_suspension]

Test `test_01_three_strikes_suspension` in `tests/test_moderation.py`. The fixture is a user with a published page and a published post. The test files three reports against that user (`content_owner_id`), calls `action_take_action_and_strike()` on each, and asserts that the strike count is 3 and `is_suspended_from_websites` is True. It then commits and polls (up to 20 rounds) for the background thread `_async_unpublish_content` to unpublish the page and post. It also cites `COMM_increment_strike_count`, `COMM_register_hook`, `COMM_action_suspend_user_websites` and `COMM_db_functions_init`. It commits real rows, so it is a real-transaction test.

### `ir_cron_notify_pending_reports` (cron record) [@ANCHOR: ir_cron_notify_pending_reports]

**What:** the scheduled action in `data/user_websites_data.xml`, named "User Websites: Notify Admins of Pending Reports". **Configuration:** model `content.violation.report`, runs as `user_websites.user_websites_service_account`, state `code`, code `model._cron_notify_pending_reports()`, every 1 day, active by default. **Effect:** at most one summary email per company per day, so administrators are not spammed per report; the browser toast is the immediate alert. Odoo's cron runner holds the usual per-job lock; a failed run is retried at the next interval. Other crons in this module: `ir_cron_reconcile_website_eligibility` every 10 minutes, `ir_cron_send_weekly_digest` weekly, `ir_cron_flush_view_counters` every 15 minutes. **Tests:** `test_cron_pending_reports` (see the entry for `cron_notify_pending_reports`).

### `content.violation.report._cron_notify_pending_reports()` [@ANCHOR: cron_notify_pending_reports]

**Purpose:** the body of the daily digest cron. **Signature:** `@api.model`, no arguments, returns `None`. **Behaviour:** as the User Websites service account it iterates that account's own `company_ids` (not every company in the database), counts reports in state `new` with `with_company(company)`, and for each company with a count above zero sends the template `email_template_pending_violations_summary` ("Action Required: N Pending Violation Reports"). The recipient is the `user_websites.company_abuse_email` system parameter, else the company email, else `admin@example.com`. The mail is queued (`force_send=False`) as the mail service account. **Errors:** a missing template is skipped silently. **Tests:** `test_08_cron_pending_reports` and `test_09_cron_pending_reports_ignores_out_of_scope_company` in `tests/test_audit_edge_cases.py`.

### `test_cron_pending_reports` [@ANCHOR: test_cron_pending_reports]

Test `test_08_cron_pending_reports` in `tests/test_audit_edge_cases.py` (its code anchor is `COMM_test_cron_pending_reports`; this name is cited from the cron record). **Given** one report in state `new`, **when** `_cron_notify_pending_reports()` runs and the ORM is flushed, **then** a `mail.mail` addressed to the abuse email (parameter, else company email, else `admin@example.com`) with a subject containing "Action Required" exists, and its body contains "unhandled content violation reports". The test then triggers the cron record once and renders the template directly. A neighbouring test proves the cron skips companies outside the service account's scope.

### `send_weekly_digest` cron record [@ANCHOR: ir_cron_send_weekly_digest]

**What:** the scheduled action "User Websites: Send Weekly Subscriptions Digest" in `data/user_websites_data.xml`. **Configuration:** model `blog.post`, user `user_websites_service_account`, code `model.send_weekly_digest()`, every 1 week, active. When one run fills a batch it re-arms itself with `_trigger()` so the backlog drains without waiting a week; a run that finds nothing resets the cursor. **Tests:** `test_cron_batching_resumption`, which triggers the record and checks that the stored cursor `user_websites.last_digest_id` is honoured. See `send_weekly_digest` for the method body.

### `blog.post.send_weekly_digest()` [@ANCHOR: send_weekly_digest]

**Purpose:** emails each follower a digest of the posts a followed user or group published in the last 7 days. **Signature:** `@api.private @api.model`, no arguments, returns `None`. **Algorithm:** read the cursor `user_websites.last_digest_id`; select up to 50 rows with a higher id from the SQL view `user_websites.weekly_digest_view` (one row per follower and owner, with comma-joined post ids); for each row with a partner email, build a token `HMAC-SHA256(secret, model-owner-partner-timestamp)` and an unsubscribe URL `/website/unsubscribe/<model>/<id>/<partner>/<timestamp>/<token>`, put it in `List-Unsubscribe` (single angle brackets, RFC 2369) and `List-Unsubscribe-Post: List-Unsubscribe=One-Click`, then queue the template mail as the mail service account. **Resumption:** a full batch of 50 stores the last id and triggers the cron again; fewer resets the cursor to 0. **Errors:** no template or no crypto secret returns early (the secret failure is logged as an error). **Tests:** `test_weekly_digest_secret`, `test_cron_batching_resumption`.

### `test_cron_batching_resumption` [@ANCHOR: test_cron_batching_resumption]

Test `test_02_cron_batching_resumption` in `tests/test_audit_edge_cases.py`, cited from the cron record's `audit-ignore-cron` comment. It triggers the cron record, resets `user_websites.last_digest_id` to 0 through `_set_system_param` (the only whitelisted way to write it), creates a published post, then sets the cursor to 999999 to simulate an interrupted batch and calls `send_weekly_digest()` directly. Because no digest row has an id above the cursor, the run must finish cleanly and reset the cursor: the test asserts the stored value is `"0"` afterwards. It proves the cursor is parsed and honoured, and the reset after a drained queue.

### `test_weekly_digest_mail_template` [@ANCHOR: test_weekly_digest_mail_template]

The mail half of `test_01_weekly_digest_and_unsubscribe_headers` in `tests/test_subscriptions.py` (code anchor `COMM_test_weekly_digest_mail_template`). With a creator, a follower and a recent published post, it runs `send_weekly_digest()` and finds the queued `mail.mail` for the follower with subject "Weekly Update from Content Creator". It asserts the mail was created by the mail service account (`zero_sudo.mail_service_internal`), that its `headers` (read with `ast.literal_eval`, never `eval`) contain `List-Unsubscribe` and `List-Unsubscribe-Post`, and that the URL maps to `/website/unsubscribe/res.partner/`. The template builds `post_links` with `Markup` so a post title cannot inject HTML.

### `test_weekly_digest_secret` [@ANCHOR: test_weekly_digest_secret]

The token half of `test_01_weekly_digest_and_unsubscribe_headers` in `tests/test_subscriptions.py`, cited from `send_weekly_digest()`. The test recovers the unsubscribe URL from the `List-Unsubscribe` header (stripping the angle brackets), opens it as an unauthenticated visitor, and asserts HTTP 200 with the text "Unsubscribed Successfully" and that the follower was removed from the creator's `message_follower_ids`. The link carries an HMAC-SHA256 token from `_get_crypto_secret()` over model, record, partner and timestamp, which the controller (`controller_unsubscribe_digest`) must verify; a wrong secret or message layout fails here. The token has no expiry check. `test_unsubscribe_secret` is the matching anchor.

### `ir_cron_flush_view_counters` (cron record) [@ANCHOR: ir_cron_flush_view_counters]

**What:** "User Websites: Flush Redis View Counters" in `data/user_websites_data.xml`. **Configuration:** model `website.page`, user `user_websites_service_account`, code `model._flush_redis_view_counters()`, every 15 minutes, active. Page views are counted in Redis (`views:<db>:page:<id>`) by `_serve_page`; this cron writes them to `website_page.view_count`. The method re-triggers the cron itself only when a run drained a full batch of 1000 keys and more remain, which ends the chain; a 2026-10 incident (about 500,000 runs a day) came from re-triggering on a non-zero SCAN cursor. **Test:** `test_cron_redis_flush`.

### `test_cron_redis_flush` [@ANCHOR: test_cron_redis_flush]

Test `test_06_cron_redis_flush_batching` in `tests/test_audit_edge_cases.py`, cited from the cron record. **Given** real Redis counters, one page with 42 views and one already at 0, **when** `_flush_redis_view_counters()` runs, **then** the first page's `view_count` rises by exactly 42, both keys are deleted from Redis, and `_trigger` is not called; a second run finds nothing and triggers nothing. Sibling tests walk a non-zero SCAN cursor and a full-batch re-trigger. It needs a running Redis and cleans its keys up afterwards.

### `user_websites_flush_view_counters(updates JSONB)` [@ANCHOR: procedure_flush_view_counters]

**Purpose:** one database round trip for all view-count increments of a flush. **Definition:** `data/procedures.xml` installs a PL/pgSQL function through a `noupdate` server action that also runs once during every module update. **Argument:** `updates`, a JSON array of `{"inc": int, "pid": int}` objects (increment and page id). **Returns:** `VOID`. **Behaviour:** for each element it runs `UPDATE website_page SET view_count = COALESCE(view_count, 0) + inc WHERE id = pid`, so unknown page ids are ignored and nothing is created. The caller, `_flush_redis_view_counters`, deletes the Redis keys first and calls this inside its transaction: a failure loses views rather than double-counting them. **Test:** `test_01_privacy_friendly_view_counter` in `tests/test_appeals_and_views.py` (a public visit, then a flush, then the raw SQL count equals 1).

### `simulation_environment` (high-speed simulation test) [@ANCHOR: simulation_environment]

Test `test_01_high_speed_full_platform_exercise` in `tests/test_simulation.py`, class `TestLongRunningSimulation`, tagged `simulation` and excluded from the standard run (`-standard`). `setUp` creates 20 users and gives the administrator the User Websites administrator group. The test loops `SIMULATION_DURATION_MINUTES` (default 60) iterations; each iteration times, with a `track()` wrapper, guest visits (`/community`, `/manual`, `/privacy`, `/terms`), a member login, `create_site` and `create_blog` POSTs, administrator report processing and appeal pardons, a GDPR export and a database flush. Afterwards it compares each operation's average with `SIMULATION_MAX_AVG_TIME` (default 0.5 s; larger allowances for the GDPR export, report processing and the flush) and logs a regression when exceeded. It commits data; run it only on a test database.

### Dropzone `user_websites_dropzone_home_header` [@ANCHOR: dropzone_home_header]

**Where:** an empty `<div id="user_websites_dropzone_home_header">` at the top of the QWeb template `user_websites.template_default_home` in `views/user_websites_templates.xml`, before the page body (`<t t-out="0"/>`). Every member or group home page created by the module calls this template, so the div appears above the page's own content. **How to use:** inherit the template and target the id with `<xpath expr="//div[@id='user_websites_dropzone_home_header']" position="inside">`. Keep the content static; the page is cached for guests. **Contract:** the id never changes; removing the div breaks dependent modules. **Test:** `test_07_navbar_rendering` in `tests/test_xpath_rendering.py` renders `/<slug>` and asserts the id is present (`test_dropzone_home_header`).

### Dropzone `user_websites_dropzone_home_footer` [@ANCHOR: dropzone_home_footer]

**Where:** the empty `<div id="user_websites_dropzone_home_footer">` at the bottom of `user_websites.template_default_home`, after the page body, in `views/user_websites_templates.xml`. **How to use:** an `<xpath expr="//div[@id='user_websites_dropzone_home_footer']" position="inside">` in an inheriting view puts markup, for example a sponsor line or a related-links block, at the end of every member home page. **Contract:** the id is stable and the div stays empty in this module. Dependent modules own what they inject, including escaping any user data they print. **Test:** `test_07_navbar_rendering` in `tests/test_xpath_rendering.py` fetches a member's home page and asserts `id="user_websites_dropzone_home_footer"` is in the response (`test_dropzone_home_footer`).

### Dropzone `user_websites_dropzone_navbar` [@ANCHOR: dropzone_navbar]

**Where:** an empty `<div id="user_websites_dropzone_navbar">` that `layout_inherit_user_navbar` (an inheritance of `website.layout` in `views/user_websites_templates.xml`) places directly after the site `<header>`, ahead of the member navbar `user_websites.user_navbar` that is called only on member sites. **Use:** inherit `website.layout` or this template and add content `inside` the div to place a banner or global notice between the site header and the member navbar. **Contract:** the id is stable; the div renders on every website page that uses `website.layout`, not only on member sites, so check `resolved_slug` if your content is member-specific. **Test:** `test_07_navbar_rendering` in `tests/test_xpath_rendering.py` (`test_dropzone_navbar`).

### Dropzone `user_websites_dropzone_templates` [@ANCHOR: dropzone_templates]

**Where:** an empty `<div id="user_websites_dropzone_templates">` that the view `portal_my_home_appeal` (an inheritance of `portal.portal_my_home`, in `views/user_websites_templates.xml`) puts before the `o_portal_my_home` block, above the account-suspended notice and the privacy and data links. **Use:** a dependent module that wants a card on the portal "My Account" page inherits that view and targets the id with `position="inside"`; this is also where new portal templates for members are registered. **Contract:** the id is stable; the page needs a logged-in portal user. **Test:** `test_05_portal_templates` in `tests/test_xpath_rendering.py` logs in a portal user, opens `/my/home` and asserts the id and the privacy text are in the HTML (`test_dropzone_templates`).

### Dropzone `user_websites_dropzone_snippets` [@ANCHOR: dropzone_snippets]

**Where:** an empty `<div id="user_websites_dropzone_snippets">` inside the `website.snippets` inheritance `user_websites_snippet_category` in `views/snippets.xml`, just before the "Profile Widgets" panel `snippet_user_websites` that the same view adds to the website builder's snippet sidebar. **Use:** inherit `website.snippets` (or this view) and add your own drag-and-drop blocks `inside` the id, or put them in the `user_websites_snippets_body` panel. **Contract:** the id is stable. Snippets are only offered to users who may open the builder, which this module never grants to members. **Test:** `test_04_snippets` in `tests/test_xpath_rendering.py` reads the combined arch of `website.snippets` and asserts `id="snippet_user_websites"` exists (`test_dropzone_snippets`).

### Dropzone `user_websites_dropzone_layout` [@ANCHOR: dropzone_layout]

**Where:** an empty `<div id="user_websites_dropzone_layout">` placed after the `<footer>` of every page by `layout_inherit_report_violation`, an inheritance of `website.layout` in `views/website_layout.xml`, followed by the call to the report-violation modal. **Use:** add global markup that must exist once per page (for example a cookie or notice container, or extra modals) by inheriting `website.layout` and targeting the id with `position="inside"`. **Contract:** the id is stable, and its position after the footer is relied on by the violation modal. **Test:** `test_06_layout_templates` in `tests/test_xpath_rendering.py` fetches `/` as a guest and asserts the modal `reportViolationModal` rendered (`test_dropzone_layout`).

### Dropzone `user_websites_dropzone_users` [@ANCHOR: dropzone_users]

**Where:** an empty `<div id="user_websites_dropzone_users">` at the top of the "User Websites" notebook page (`name="user_websites_settings"`) that `view_users_form_user_websites` adds to the back-office user form (`base.view_users_form`, `views/res_users_views.xml`). The page also shows `website_slug`, `website_page_limit`, `privacy_show_in_directory` and, for administrators, strike count and suspension controls. **Use:** inherit the form and add your own per-user website settings `inside` the id. **Contract:** the id is stable; fields you add must be readable by whoever opens the user form. **Test:** `test_02_res_users` in `tests/test_xpath_rendering.py` loads the compiled form arch and asserts `name="user_websites_settings"` (`test_dropzone_users`).

### Dropzone `user_websites_dropzone_blog_post` [@ANCHOR: dropzone_blog_post]

**Where:** an empty `<div id="user_websites_dropzone_blog_post">` in the back-office blog post form, inserted by `view_blog_post_form_inherit_user_websites` (`views/blog_post_views.xml`) after `blog_id`, right after the `owner_user_id` and `user_websites_group_id` fields it adds. **Use:** inherit `website_blog.view_blog_post_form` and add fields for the proxy owner, such as moderation notes, `inside` the id. **Contract:** the id is stable. Group selection is `no_create` on purpose, because groups are made on their own form. **Test:** `test_03_blog_post` in `tests/test_xpath_rendering.py` loads the form arch and asserts `name="user_websites_group_id"` is present (`test_dropzone_blog_post`).

### Dropzone `user_websites_dropzone_navbar_actions` [@ANCHOR: dropzone_navbar_actions]

**Where:** an empty `<div id="user_websites_dropzone_navbar_actions">` at the end of the collapsible part (`#userNavbarNav`) of the member navbar template `user_websites.user_navbar`, after the list of Home, Blog and member menu pages (`views/user_websites_templates.xml`). **Use:** inherit `user_websites.user_navbar` and add buttons, such as a "Contact" or "Donate" action, `inside` the id; they sit on the right side of every member site's navbar. **Contract:** the id is stable and the navbar renders only on member sites (`member_site` true). Use `resolved_slug`, `profile_user` and `profile_group` from the QWeb context. **Test:** `test_07_navbar_rendering` in `tests/test_xpath_rendering.py` (`test_dropzone_navbar_actions`).

### Dropzone `user_websites_dropzone_directory_card` [@ANCHOR: dropzone_directory_card]

**Where:** an empty `<div id="user_websites_dropzone_directory_card" class="mt-3">` inside each entry card of the `/community` directory template (`views/user_websites_templates.xml`), under the "Visit Home" and "View Blog" buttons; the card shows the member's name and total view count. **Use:** inherit the directory template and add per-entry badges or links `inside` the id; the loop variable is `entry` (a `user_websites.public_directory_view` row with `user_id`, `name`, `website_slug`, `view_count`). **Contract:** the id is stable and repeats once per entry, so it must not be used with an `id`-based JavaScript lookup. **Test:** none cites this anchor yet (the view comment names `test_dropzone_directory_card`, which does not exist): to-do, add a `/community` render assertion.

### Dropzone for the global settings form [@ANCHOR: dropzone_settings]

**Where:** an empty `<div id="user_websites_dropzone_settings">` that `res_config_settings_view_form_inherit_user_websites` (`views/res_config_settings_views.xml`) puts inside the `//form` of `base.res_config_settings_view_form`, before the `<app name="user_websites">` block that holds "Manage Administrators", the global page limit and the abuse-report email. **Use:** inherit `base.res_config_settings_view_form`, target the form with the modern `<app>`, `<block>` and `<setting>` tags, and add settings `inside` the app, not inside this div; the div is the stable marker. The old `user_websites_settings_dropzone` is prohibited. **Contract:** the view is only for settings administrators. **Test:** `test_01_res_config_settings` in `tests/test_xpath_rendering.py` asserts `data-key="user_websites"` is in the compiled arch (`test_dropzone_settings`).

### `GET /community` (community directory) [@ANCHOR: UX_COMMUNITY_DIRECTORY]

**Purpose:** the public directory of members who opted in with `privacy_show_in_directory`. **Route:** `UserWebsitesController.community_directory(**kwargs)`, `auth="public"`, `website=True`, GET; the user-experience anchor is also written in `data/documentation.html`. **Returns:** the `user_websites.community_directory` page with `entries`, at most 1000 rows read from the SQL view `user_websites.public_directory_view` as the service account: active users with a slug, not service accounts, not suspended, not website-ineligible, each with name, slug, last update and summed page and post views. A reader who has not opted in is never listed. **Errors:** none of its own. **Side effects:** none; read-only, no locks. **Tests:** `test_03_community_directory_rendering`, `test_03b_community_directory_internal_user` and `test_04_community_directory_privacy_respect` in `tests/test_controllers.py`, and the tour `community_directory_tour.js`.

### Home page route `/<slug>` [@ANCHOR: controller_user_websites_home]

**Purpose:** serve a member or group site's home. **Route:** `user_home_fallback(website_slug, **kwargs)`, `auth="public"`, `website=True`, hidden from the sitemap. **Behaviour:** resolves the slug as the service account with the cached `get_record_by_slug` for users and groups. Unknown slug: a retired-slug redirect, else Odoo's stock page lookup, else 404. A suspended or not publicly visible owner gets 404. A different spelling of the slug gets a 301 to the canonical lower-case address. A published home page is served by Odoo after one Redis `INCR` of the view counter (skipped for administrators; a Redis failure is logged only). With no page, the owner sees the "Create Your Site" placeholder, and everyone else a plain placeholder. **Tests:** `test_group_site_routing` in `tests/test_lifecycle_and_groups.py`, `test_01_home_served_at_slug` to `test_06_rewrite_redirects_still_fire` in `tests/test_member_site_home_url.py`, and the tour `create_site_tour.js`.

### Legacy home redirect `/<slug>/home` [@ANCHOR: user_websites:COMM_user_home_legacy_redirect]

**Purpose:** keeps old links, bookmarks and search-engine entries working after the home moved from `/<slug>/home` to `/<slug>`. **Route:** `user_home_legacy_redirect(website_slug, **kwargs)`, `auth="public"`, hidden from the sitemap. **Behaviour:** resolves the slug as a user, then a group; when found it answers HTTP 301 to `"/" + quote(record.website_slug, safe="")` with `local=True`, built from the stored slug (lower-case, `[a-z0-9-]`), never from the request path, so it cannot be steered to another host and a single segment cannot loop. When no record exists it tries a retired-slug redirect, then Odoo's stock fallback, then 404. **Returns:** a redirect response, a page, or `NotFound`. No writes. **Tests:** `test_02_legacy_home_is_permanent_redirect` and `test_03_legacy_redirect_has_no_open_redirect_or_loop` in `tests/test_member_site_home_url.py`.

### Home route fallback `user_home_fallback` [@ANCHOR: user_websites:COMM_user_home_fallback]

The method itself: `UserWebsitesController.user_home_fallback(website_slug, **kwargs)` for `/<slug>`, described in the entry for the home route (`controller_user_websites_home`). Points to keep: it is the only handler for a single-segment path, so stock pages such as `/aboutus` and website rewrites still work through its fallback to `ir.http._serve_fallback`; for an existing page it deliberately does not use the page cache path, so a page unpublished by a suspension disappears at once; the owner or a group member gets `home_state` (none, unpublished, deleted, blocked) from `_owner_home`, so the placeholder and the create button never disagree. **Tests:** `tests/test_member_site_home_url.py` and `test_01_privacy_friendly_view_counter` in `tests/test_appeals_and_views.py`.

### Site creation `POST /<slug>/create_site` [@ANCHOR: UX_CREATE_SITE]

**Purpose:** Just-In-Time creation of the home page, after the owner presses "Create Your Site". **Route:** `create_site(website_slug, **kwargs)`, `auth="user"`, POST only, `csrf=True`. **Checks:** unknown slug, a user who is not the slug's owner, a suspended owner, a group the caller is not a member of: all 404. An owner who is not yet eligible gets a 303 to `/my/site/<slug>`. **Behaviour:** `_owner_home` finds an existing home: blocked goes to `/my/site/<slug>?error=home_blocked`; deleted is restored and published; unpublished is published; each redirects to `/<slug>`. Otherwise it creates a `website.page` as the service account with `owner_user_id` or `user_websites_group_id`, a QWeb arch calling `user_websites.template_default_home`, and the owner's name XML-escaped, then redirects to it. **Tests:** `test_02_group_member_access` and `test_02b_create_site_escapes_malicious_name` in `tests/test_lifecycle_and_groups.py`, and `create_site_tour.js`.

### Blog index route `/<slug>/blog` [@ANCHOR: controller_user_blog_index]

**Purpose:** a member's or group's blog listing, `/<slug>/blog` and `/<slug>/blog/page/<n>`. **Route:** `user_blog_index(website_slug, tag=None, search=None, date_begin=None, date_end=None, page=1, **kwargs)`, `auth="public"`. **Behaviour:** resolves the slug like the home route (retired slug redirects to `/blog` of the live one; unknown, suspended or not visible gives 404). It reads up to 100 blogs and 100 posts as the service account, but only the owner or a group member sees drafts: everyone else gets a domain with `is_published = True`, because the service account's own rule would otherwise reveal unpublished posts. With no blog and no post it renders the placeholder (with the create button for the owner), else `user_websites.blog_index`. The filter arguments are accepted but the pager is fixed. **Test:** `create_blog_tour.js`.

### Blog creation `POST /<slug>/create_blog` [@ANCHOR: UX_CREATE_BLOG_POST]

**Purpose:** creates the member's or group's `blog.blog` the first time (the "Create Your Blog" button). The anchor is the user-experience name shared with blog post creation in `data/documentation.html`. **Route:** `create_blog(website_slug, **kwargs)`, `auth="user"`, POST only, `csrf=True`. **Checks:** unknown slug, a caller who is not the owner, a suspended site, or a non-member of the group give 404; an ineligible owner is redirected (303) to `/my/site/<slug>`. **Behaviour:** as the service account it creates `blog.blog` named `<Name> Blog` (`_new_blog_name`) with the owner or group, and the blog-count limit (5 by default) applies in `create()`. **Returns:** a redirect to `/<slug>/blog`. **Tests:** `test_08_group_inverse_relationships` in `tests/test_lifecycle_and_groups.py` and `create_blog_tour.js`.

### Documentation route `/user-websites/documentation` [@ANCHOR: user_websites:documentation]

**Purpose:** a stable address for the module's user documentation. **Route:** `documentation(**kwargs)`, `auth="user"`, `website=True`. **Behaviour:** as the requesting user (not the service account, so portal access rights apply) it searches `knowledge.article` for a name starting with "User Websites Documentation" (case-insensitive); when the article has a `website_url` it redirects there. The article is installed from `data/documentation.html` by zero_sudo's knowledge bootstrap and is published. If it cannot be found it renders the fallback template `user_websites.documentation_page`. **Returns:** a redirect or a page; no errors. No writes. **Test:** `test_02_documentation_route_authenticated` in `tests/test_documentation.py` (code anchor `COMM_documentation`, test anchor `test_documentation_route`).

### Appeal submission `POST /website/submit_appeal` [@ANCHOR: UX_SUBMIT_APPEAL]

**Purpose:** lets a suspended member, or a member of a suspended group, ask for the suspension to be lifted. **Route:** `submit_appeal(reason="", group_id=None, **post)`, `auth="user"`, POST only, `csrf=True`. **Arguments:** `reason` (text, required) and `group_id` (optional id of a group the caller belongs to). **Behaviour:** an empty reason redirects to `/my/home?error=missing_reason`; with `group_id` the caller must be in `member_ids`, else `?error=appeal_failed`; otherwise it creates a `content.violation.appeal` in state `new` as the service account. **Returns:** redirect to `/my/home?appeal_submitted=1`, which the toast script shows. A `KeyError` or `ValueError` is logged and redirects with `appeal_failed`. Approval or rejection is an administrator action elsewhere. **Tests:** `test_02_submit_and_approve_appeal` in `tests/test_appeals_and_views.py`, and `moderation_appeal_tour.js`.

### Subscription `POST /<slug>/subscribe` [@ANCHOR: UX_SUBSCRIBE]

**Purpose:** subscribe the signed-in visitor to the weekly digest of a member or group. **Route:** `subscribe(website_slug, **kwargs)`, `auth="user"`, POST only, `csrf=True`. **Behaviour:** resolves the slug as a user or group (404 if neither). For a user the follower record is the user's partner (`res.partner`), for a group the `user.websites.group` itself; as the service account it calls `message_subscribe` with the caller's partner id. Subscribing twice does nothing more. **Returns:** JSON `{"success": true}` with content type `application/json`. The follower rows feed the digest SQL view used by `send_weekly_digest`. **Test:** `test_03_subscribe_to_site` in `tests/test_subscriptions.py`.

### Unsubscribe `/website/unsubscribe/...` [@ANCHOR: controller_unsubscribe_digest]

**Purpose:** one-click unsubscribe from a digest, reachable without login from the email header. **Route:** `unsubscribe(model, record_id, partner_id, timestamp, token, **kwargs)`, `auth="public"`, path `/website/unsubscribe/<model>/<record_id>/<partner_id>/<timestamp>/<token>`. **Checks:** `model` must be `res.partner` or `user.websites.group` and the record must exist, else HTTP 403 JSON `{"error": "Forbidden"}`; the database crypto secret must be set; the token must equal `HMAC-SHA256(secret, "<model>-<record_id>-<partner_id>-<timestamp>")` in hex, compared with `hmac.compare_digest` (constant time). **Effect:** `message_unsubscribe([partner_id])` as the service account, then renders `user_websites.unsubscribe_success`. The timestamp is signed but its age is not checked, so a link does not expire. **Tests:** `test_unsubscribe_secret` (an invalid token leaves the follower subscribed) and the success path in `tests/test_subscriptions.py`.

### `GET /api/v1/user_websites/pending_reports` [@ANCHOR: api_pending_reports]

**Purpose:** the data source of the administrator toast and the badge for the moderation queue. **Route:** `pending_reports(**kwargs)`, `auth="public"`, `website=True`, `sitemap=False` (kept out of the public sitemap on purpose). **Authorisation:** only a logged-in user in `group_user_websites_administrator` or `base.group_system`. **Returns:** HTTP 200 always. Moderators get `{"count": int, "latest_id": int, "url": "/odoo/action-user_websites.action_content_violation_report_pending"}` (count of reports in state `new`, capped at 1000 in the search). Everybody else gets `{"count": 0, "error": "Forbidden"}` and no `url` or `latest_id`; the 200 status is so the page's `fetch` does not raise. Read-only as the service account. **Tests:** `test_01_api_pending_reports_admin_access` and `test_02_api_pending_reports_unauthorized_access` in `tests/test_controllers.py`.

### `test_admin_violation_toast_rpc` [@ANCHOR: test_admin_violation_toast_rpc]

The server half of the administrator toast: `test_01_api_pending_reports_admin_access` in `tests/test_controllers.py` (the anchor is declared inside its docstring). It logs in as an administrator with at least one pending report and GETs `/api/v1/user_websites/pending_reports`. It asserts HTTP 200, a JSON object with `count` at least 1, `latest_id` above 0 and `url` equal to the back-office pending-queue action, and that the action exists. The script `AdminViolationToast._checkPendingReports()` in `static/src/js/toast_notifications.js` depends on exactly those keys. The companion test `test_02` proves a non-administrator gets the `Forbidden` payload without `url` or `latest_id`.

### `toast_notifications_logic` (URL toast widget) [@ANCHOR: toast_notifications_logic]

**What:** `UrlToastNotification._checkUrlForNotifications()` in `static/src/js/toast_notifications.js`, a public interaction on `#wrapwrap` started on every page. **Input:** the query string. **Behaviour:** `report_error=<reason>` shows a danger toast "Report not sent" with the text from the `REPORT_ERRORS` table (unknown reasons use `default`); `report_submitted`, `appeal_submitted`, `subscribed` and `erased` show a success toast with the matching text. The first match wins. Then it removes the query string with `history.replaceState`, so a reload does not repeat the toast, and the back button is unchanged. **Side effects:** a non-sticky notification through Odoo's notification service; no network. **Errors:** none. **Test:** the tour `toast_notifications_tour` (`test_tour_toast_notifications`) opens `/?report_submitted=1` and waits for the toast.

### `admin_toast_logic` (moderator toast) [@ANCHOR: admin_toast_logic]

**What:** `AdminViolationToast._checkPendingReports()` in `static/src/js/toast_notifications.js`, a public interaction on every page. **Behaviour:** it fetches `/api/v1/user_websites/pending_reports`; a non-OK response or any error is ignored silently. `shouldShowToast(seen, data, now)` shows it when `count > 0` and it was never seen, the stored record is older than 12 hours (or in the future), or the count or newest report id grew. The sticky "Pending Moderation" toast has a "Review reports" button (only when `url` starts with `/`) and "Dismiss". `readSeen` and `writeSeen` keep `{count, latest_id, ts}` in `localStorage` under `admin_violation_toast_seen` and tolerate blocked storage. **Tests:** `test_admin_violation_toast_rpc`, `test_tour_toast_notifications`, and `tests/toast_notifications.test.js`.

### `test_tour_toast_notifications` [@ANCHOR: test_tour_toast_notifications]

The browser tour `toast_notifications_tour` in `static/tests/tours/toast_notifications_tour.js`, run by `test_02_toast_notifications_tour` in `tests/test_ui_tours.py`. It opens `/?report_submitted=1`, dismisses the cookie bar and waits for a `.o_notification_manager .o_notification` element, proving that the URL toast widget turns the query parameter into a visible notification. It covers `toast_notifications_logic` directly; `admin_toast_logic` is cited here too because both interactions live in one file, though the administrator path needs a logged-in moderator and is covered by the unit tests in `static/tests/toast_notifications.test.js`. Needs a headless browser.

### Privacy dashboard `GET /my/privacy` [@ANCHOR: controller_my_privacy_dashboard]

**Purpose:** the member's page for the data-protection rights this module offers: export of data and erasure of content. **Route:** `privacy_dashboard(**kwargs)`, `auth="user"`, `website=True`. **Returns:** the template `user_websites.portal_my_privacy`, with the "Download My Data" export form (`/my/privacy/export`) and the "Right to Erasure" card that posts to `/my/privacy/delete_content`. Anonymous visitors are sent to login by Odoo. No arguments are read, nothing is written. The export is read through the member's own session, so nobody can open another person's data here. **Test:** the tour `gdpr_privacy_tour` (`static/tests/tours/gdpr_privacy_tour.js`, started by `test_03_gdpr_privacy_tour` in `tests/test_ui_tours.py`) checks the header, the export button and the erasure flow.

### Data export `/my/privacy/export` [@ANCHOR: UX_GDPR_EXPORT]

**Purpose:** download of the member's data as a JSON file. **Route:** `privacy_export(**kwargs)`, `auth="user"`, `website=True`. **Behaviour:** calls `request.env.user._get_gdpr_export_data()` for the plain keys and `_get_gdpr_streamed_keys()` for the generators, and streams one JSON object, writing each list element as it is produced, so a large account does not fill memory. **Response headers:** `Content-Type: application/json`, `Content-Disposition: attachment; filename="gdpr_export.json"`, `Cache-Control: private, no-store`, so no cache keeps the file. The sibling `/my/privacy/export.zip` mints a single-use token and redirects to the separate export daemon. **Tests:** `test_01_data_portability_export` in `tests/test_privacy_gdpr.py` (200, JSON, attachment, no-store) and `gdpr_privacy_tour.js`.

### Content erasure `POST /my/privacy/delete_content` [@ANCHOR: UX_GDPR_ERASURE]

**Purpose:** the "delete my content" button. **Route:** `privacy_delete_content(**kwargs)`, `auth="user"`, POST only, `csrf=True`. **Behaviour:** calls `request.env.user._execute_gdpr_erasure()` (see `gdpr_sudo_erasure`), which hard-deletes the member's pages, posts, blogs and uploaded images in batches of 5000 under the GDPR service account, within the request, and which also deactivates the account. Because the account is then inactive, it redirects to the public page `/privacy/erased` rather than a login-gated page. **Errors:** errors from the erasure propagate; there is no undo. **Tests:** `test_02_right_to_erasure` in `tests/test_privacy_gdpr.py` and the tour `gdpr_privacy_tour.js`, which clicks the erasure button.

### Erasure confirmation `GET /privacy/erased` [@ANCHOR: user_websites:COMM_privacy_erased]

**Purpose:** a static confirmation page that an erased member can still open after the account is deactivated. **Route:** `privacy_erased(**kwargs)`, `auth="public"`, `website=True`. **Behaviour:** renders `user_websites.privacy_erased` ("Your data has been erased", element `user_websites_erasure_confirmed`) for everyone, identically. It reads no input: the query string is ignored, nothing is reflected, there is no redirect target, so the page cannot tell anyone whether an account ever existed. **Returns:** HTTP 200. No writes. **Tests:** the test anchored `test_privacy_erased_public_page` and `test_02_right_to_erasure` in `tests/test_privacy_gdpr.py`, and the end of the tour `gdpr_privacy_tour.js`.

### Report limits `REPORT_LIMITS` and `REPORT_MEMBER_LIMITS` [@ANCHOR: user_websites:report_limits]

**What:** the rate-limit tables in `controllers/main.py` that `report_violation()` passes to `form_shield.check_form(..., limits=...)`. Each row is `(counter name, allowed count, window in seconds)`. **Guests (`REPORT_LIMITS`):** `("subject", 5, 3600)` five reports an hour per visitor network, `("target", 10, 86400)` ten a day against one reported page, `("global", 100, 3600)` a hundred an hour for the whole site. **Members (`REPORT_MEMBER_LIMITS`):** the same but 30 an hour per user. **Why these numbers:** a page that really is a problem is reported by a handful of people, not hundreds; the "subject" counter is keyed on the client network the shield derives (behind the Cloudflare tunnel `remote_addr` is always loopback, which once made every visitor share one counter). **Result:** over a limit the form answers HTTP 429 with a notice; if the counter store (Redis) is down, 503. Related constants: `MIN_DESCRIPTION_CHARS` 10, `MAX_URLS_FROM_GUEST` 2. **Tests:** `test_03c_report_violation_is_rate_limited` (one allowed, the second gets 429) and `test_report_violation_limits_and_unavailable_store`.

### `_bare_host(host)` [@ANCHOR: user_websites:bare_host]

**Purpose:** compares host names without the cosmetic `www.` prefix. **Argument:** `host`, a string or `None`. **Returns:** the host lower-cased, with any trailing dot removed and a leading `www.` removed; `""` for `None` or an empty value. Examples: `"WWW.Example.org."` gives `"example.org"`. **Errors and effects:** none, a pure function. It is used by `_own_site_hosts()` to build the allowed set and by `normalize_report_target()` to compare the reported host with that set, so `http://www.hams.com/x` and `https://hams.com/x` are treated as the same site. **Test:** `test_normalize_report_target` in `tests/test_report_shield.py` (accepted and refused host spellings).

### `_own_site_hosts()` [@ANCHOR: user_websites:report_own_site_hosts]

**Purpose:** the set of host names a violation report may be about, so a visitor cannot use the report form to file a report about, or be redirected to, another site. **Arguments:** none; it reads the current `request`. **Returns:** a `set` of bare host names (no port, no `www.`): the request's own host, the host of `web.base.url`, and the `domain` of every `website` record (read with `search([], limit=500)`). Blank names and names that do not parse are dropped. **Errors:** none; a missing parameter simply adds nothing. **Side effects:** two small reads per report, no writes, no cache. **Test:** `test_normalize_report_target` in `tests/test_report_shield.py` sets a website domain (`https://shield-test.example`) and checks that its pages are accepted and look-alikes are refused.

### `normalize_report_target(raw, hosts)` [@ANCHOR: user_websites:normalize_report_target]

**Purpose:** turns the posted "URL of the offending page" into one canonical form, or refuses it. **Arguments:** `raw` (the posted string or the Referer; `None` allowed) and `hosts` (the set from `_own_site_hosts()`). **Returns:** a string `origin + path + query`, or `None`. **Rules:** surrounding spaces stripped; empty, longer than 2048 characters, or any control character gives `None`; user names and passwords in the URL give `None`; a scheme must be `http` or `https` with a host in `hosts` (`www.` ignored, host lower-cased, http and https accepted alike because the tunnel makes production requests look like http); a bare path must start with a single `/` (`//host` is refused) and takes the current request's origin; the fragment is dropped. **Errors:** a `ValueError` from the URL parser returns `None`. Pure apart from reading `request.httprequest.host_url`. **Tests:** `test_normalize_report_target`, `test_03d_report_violation_refuses_another_sites_url`.

### `_local_part_ok(address)` [@ANCHOR: user_websites:local_part_ok]

**Purpose:** an extra check on a guest reporter's address, after `email_normalize(strict=True)` has accepted it, to refuse the dotted-gmail style strings used by form-spam. **Argument:** `address`, an address that already passed normalisation. **Returns:** `True` unless the local part (the text before the first `@`) starts with a dot, ends with a dot, or contains two dots in a row. Examples: `a.b@x.org` is fine; `.a@x.org`, `a.@x.org` and `a..b@x.org` are refused. No errors, no effects. It is called by `report_problem()` only for guests, and a refusal is reported to the visitor as `report_error=bad_email`. **Test:** `test_report_problem_reasons` in `tests/test_report_shield.py` (the cases `.a@example.com`, `a..b@example.com`, and a trailing-dot address that arrived as report 7).

### `report_problem(target, description, email, member)` [@ANCHOR: user_websites:report_problem]

**Purpose:** refuses a report on its content alone, before it is stored. It is deliberately structural: it never judges whether the complaint is right. **Arguments:** `target` (the normalized URL or `None`), `description` (text), `email` (string), `member` (True when the reporter is logged in). **Returns:** `None` when the report is acceptable, else a reason code: `foreign_target` (no target), `short_description` (under 10 characters after stripping), `gibberish` (one unbroken random-looking token of 16 or more characters containing lower case, upper case and digits), `too_many_urls` (a guest's text with more than 2 `http://`, `https://` or `www.` links), `bad_email` (a guest's address fails strict normalisation or `_local_part_ok`). Members skip the link and address checks. **Side effects:** none here; the caller counts the refusal by reason with `form.shield.refuse` and redirects with `report_error=<reason>`. **Tests:** `test_report_problem_reasons`.

### `_reported_path(reported_url)` [@ANCHOR: user_websites:reported_path]

**Purpose:** makes the return address after a report safe against open redirects. **Argument:** `reported_url`, an untrusted string (posted field or Referer). **Returns:** the path of that URL (never its scheme or host), or `"/"` when it is empty or unparsable, with exactly one leading slash: `"/" + path.lstrip("/\\")`, so a path that arrives as `//evil.example` or `/\evil.example`, which a browser reads as protocol-relative, cannot leave the site. Note that only the path is kept; the query string of the reported URL is not (the docstring of the caller says otherwise). **Errors:** a `ValueError` from `urlparse` is caught and gives `"/"`. Pure function. **Tests:** `test_normalize_report_target` (the `//evil.example/x` case, where the redirect must not start with `//`).

### `_report_violation_redirect_target(reported_url, query_param)` [@ANCHOR: user_websites:report_violation_redirect_target]

**Purpose:** where `report_violation()` sends the visitor after a submission, so the toast script can show the result. **Arguments:** `reported_url` (untrusted) and `query_param` (a fixed string such as `report_submitted=1`, `report_error=missing_fields` or `report_error=bad_email`, always chosen by the server). **Returns:** `_reported_path(reported_url)` plus the parameter, joined with `?`, or `&` when the path already holds a query. Because only a path is used, Odoo's `request.redirect` resolves it against the current site whatever host the field claimed, so the redirect is same-origin by construction. **Errors:** none. **Tests:** `test_report_violation_redirects_back_to_reported_page` (the redirect returns to the page) and the shield tests, which assert that `Location` starts with `/` and not `//`.

### `GET /website/report_violation` (question form) [@ANCHOR: user_websites:report_violation_page]

**Purpose:** the report form as a page of its own, for a visitor whose browser runs no scripts (the Bootstrap modal cannot open) or cannot run the proof of work (the modal links here). **Route:** `report_violation_form(url="", **_ignored)`, `auth="public"`, GET, `website=True`, hidden from the sitemap. **Argument:** `url`, the page being reported, else the `Referer` header. **Returns:** `_render_report_page(reported)`: the template `user_websites.report_violation_page` with a plain-text question issued by `form.shield.issue("q")`, hidden `csrf_token` and `url`, an email field for guests, and the description field. **Caching:** a question is issued per request, so the response carries `Cache-Control: no-store`. No writes. **Tests:** `test_question_form_page_renders_accessible_question` in `tests/test_report_shield.py`.

### `_render_report_page(url, description="", email="", notice="", status=200)` [@ANCHOR: user_websites:render_report_page]

**Purpose:** one place that renders the question form, used for the first visit and for every refusal that needs a retry. **Arguments:** `url` (the reported page), `description` and `email` (what the visitor typed, so it is not lost), `notice` (a translated message shown in a warning box), `status` (HTTP status, 200 by default; 429 and 503 for limits). **Returns:** the rendered `user_websites.report_violation_page` response with the given status and `Cache-Control: no-store`. **Side effects:** calls `form.shield.issue("q")`, which signs a fresh question challenge tied to the caller's network; nothing is stored. **Errors:** none of its own. **Test:** `test_report_violation_shield_outcomes` in `tests/test_report_shield.py` (the question page keeps the typed description and stores no report).

### `_report_refusal(verdict, url, description, email)` [@ANCHOR: user_websites:report_refusal]

**Purpose:** chooses the response when the shield refuses a submission. **Arguments:** `verdict` (a `form_shield` verdict with an `outcome`), and the visitor's `url`, `description` and `email`. **Returns:** for outcome `QUESTION`, the question page with the notice "Please answer this short question to send your report."; for `LIMITED`, the same page with HTTP 429 and "Too many reports from your network..."; for `UNAVAILABLE`, HTTP 503 with "Reporting is temporarily unavailable..."; for any other outcome (a silent or retry refusal) `form_shield.refusal_response(...)`, which tells a program that it succeeded (redirect with `report_submitted=1`) and tells a person what to retry. **Why:** a bot that gets useful feedback learns to adapt, a person keeps what they typed. No database writes. **Test:** `test_report_violation_shield_outcomes`.

### Report submission `POST /website/report_violation` [@ANCHOR: user_websites:UX_REPORT_VIOLATION]

**Purpose:** files a content-violation report about a page of this site. **Route:** `report_violation(url, reason, description, email, website_honeypot, **post)`, `auth="public"`, POST only, `csrf=True`. **Steps:** a filled `website_honeypot` is refused silently (reported as success); a missing URL (the Referer is the fallback) or description gives `report_error=missing_fields`; `normalize_report_target` and `form.shield.check_form("report_violation", ...)` run (members use the member limits); `report_problem` runs; the description is cut to 5000 characters; as the service account it resolves the first path segment as a user or group slug and creates a `content.violation.report` (state `new`) with `target_url`, `description`, `reported_by_email`, `reported_by_user_id` for a signed-in reporter (from the session, never the form), and `content_owner_id` or `content_group_id`. **Returns:** a redirect with `report_submitted=1` or a `report_error=<reason>`. **Tests:** `test_report_violation_honeypot_rejected`, `test_report_violation_records_logged_in_reporter`, `test_tour_violation_report`.

### Violation modal reset `violation_report_logic` [@ANCHOR: violation_report_logic]

**What:** `ViolationReportModal._onModalShow(ev)` in `static/src/js/violation_report.js`, a public interaction on `.user-websites-report-container` whose `start()` listens for the Bootstrap `show.bs.modal` event of `#reportViolationModal`. **Behaviour:** when the modal opens, the button that triggered it (`ev.relatedTarget`) supplies `data-url`, which is written into the modal's `input[name="url"]`; the description textarea and the email input are cleared so a previous report's text never carries over. If the event has no triggering button (a programmatic open) nothing changes. **Input trust:** `data-url` is untrusted and is re-validated on the server by `normalize_report_target`. No network calls. **Test:** the tour `test_tour_violation_report` (`static/tests/tours/violation_report_tour.js`) opens the modal, fills it and checks the stored report. The server side is `UX_REPORT_VIOLATION`.

### `test_tour_violation_report` [@ANCHOR: test_tour_violation_report]

The browser tour `test_tour_violation_report` in `static/tests/tours/violation_report_tour.js`, started by a test in `tests/test_ui_tours.py` on a member's page (a second tour, `test_tour_violation_report_edge_cached`, covers the cached page). A guest opens the report modal, types a description and the address `normal-page-tour@example.com`, waits for the shield's polite live region to say it is verifying, then sends; the browser's Web Worker solves the proof of work at production difficulty. Afterwards the test commits and searches for a `content.violation.report` with that reporter email. It also cites the same anchor from the controller's `creation_failed` branch. Needs a headless browser.

### Shield checks inside `report_violation` [@ANCHOR: user_websites:report_violation_shield]

**What:** the call `shield.check_form("report_violation", post, target=target, limits=...)` in `report_violation()` and its handling. **Inputs:** the posted fields (`shield_response` for the proof of work, or `shield_challenge` and `shield_answer` for the question form, and the shield's own honeypot `fs_homepage`), the normalized `target` for the per-page counter, and the guest or member limit table. **Result:** a verdict: accepted, or refused with outcome `QUESTION` (no answer: show the question), `RETRY` (expired or the network changed: please resend), `LIMITED` (429), `UNAVAILABLE` (503) or `SILENT` (a program: told it succeeded). Members do not need a challenge. **Order:** the shield runs before `report_problem`, so a program that cannot solve the proof of work never reaches the structural checks. **Tests:** `test_report_violation_shield_outcomes`, `test_member_reports_without_a_challenge` (a member's report is stored without a challenge; a filled honeypot is discarded).

### Shield fields in the report form [@ANCHOR: user_websites:report_violation_shield_fields]

**What:** the `<t t-call="form_shield.fields">` in the `report_violation_modal` template of `views/user_websites_templates.xml`, which adds the hidden and visible fields the shield needs: the proof-of-work response and challenge, the shield's own honeypot, a polite `role="status"` live region (`data-form-shield-status`) that tells screen-reader users it is verifying, and the link to the question page `/website/report_violation` (set through `fallback_url`). **Detail:** the template sets `issued` to `False` on purpose, because a page around the modal may use that name for something else (an onboarding page once crashed with a 500). The modal's own `website_honeypot` field, hidden from people, is separate. **Tests:** the tour `violation_report_tour.js` waits for the live region, cites `form_shield:fields_template`, and presses Send right away; the form must hold the post until the worker has solved the challenge.

### Owner toolbar [@ANCHOR: user_websites:UX_OWNER_TOOLBAR]

**What:** the QWeb template `owner_toolbar` in `views/owner_site_templates.xml`, rendered above the site's navbar by `user_navbar` for the owner (or a member of the owning group) on every page of that site. **Content:** "Manage site" (`/my/site/<slug>`), "New page", a "New blog post" POST button (`/<slug>/create_blog_post`), "Edit this page" or "Edit this post" for the page being viewed, "Create your home page" on a missing home, Images, Privacy, "Publish this page" or "Unpublish this page", "Delete" (not for the home page), and "View as visitor" (`?visitor=1`, which hides the bar). Every POST button carries a CSRF token. **Rules:** an internal user sees it on their own slug only; other members and guests never see it. **Tests:** `test_04_toolbar_visible_to_owner_only` and `test_05_toolbar_for_internal_user_on_own_slug_only` in `tests/test_owner_site.py`, and `owner_site_tour.js`.

### Owner dashboard `/my/site/<slug>` [@ANCHOR: user_websites:COMM_owner_site_dashboard]

**Purpose:** the owner's overview: pages, posts, deleted items, images and settings links. **Route:** `owner_site_dashboard(site_slug, **kwargs)`, `auth="user"`, `website=True`, hidden from the sitemap, wrapped by `answers_site_response`. **Behaviour:** `_resolve_site` decides access (see `owner_resolve_site`); then, as the service account, it reads up to 200 pages (by URL), 200 posts (newest first), up to 100 deleted pages and 100 deleted posts (`deleted_by_owner` set, archived rows included), and up to 100 image attachments of the site. It passes `has_home` (a page at `/<slug>` exists), the image limit and the size limit in MB to `user_websites.owner_site_dashboard`. **Returns:** the rendered page; 404 for anybody but the owner or a group member; 403 explaining pages for a suspended or ineligible owner. Read-only. **Tests:** `test_08_dashboard_lists_content`, `test_07_dashboard_access`.

### Owner entry `/my/site` [@ANCHOR: user_websites:COMM_owner_site_entry]

**Purpose:** a fixed address that sends a signed-in member to their own dashboard. **Route:** `owner_site_entry(**kwargs)`, `auth="user"`, `website=True`, hidden from the sitemap. **Behaviour:** reads `request.env.user.website_slug`; with no slug it redirects to `/my/home`, otherwise it answers HTTP 303 to `/my/site/<quoted slug>`. It takes no argument that influences the target, so it cannot be steered elsewhere. **Returns:** a redirect; no errors, no writes. **Tests:** `test_08_dashboard_lists_content` (the final assertion opens `/my/site` and expects 200 after the redirect) and `test_05_toolbar_for_internal_user_on_own_slug_only`.

### Page editor `/my/site/<slug>/page/new|<id>` [@ANCHOR: user_websites:UX_OWNER_PAGE_EDITOR]

**What:** the `owner_page_edit` template in `views/owner_site_templates.xml`, rendered by `owner_page_new` and `owner_page_edit`. **Fields:** a title (required, at most 120 characters), a page address (`/<slug>/<address>`, lower-case letters, digits and hyphens, read-only once the page exists, absent for the home page), the WYSIWYG body (template `rich_editor`, upload address `/my/site/<slug>/image`), a "Published" checkbox, and Save, Back, Delete (not for the home page) and "View page" buttons. Error codes `title`, `address`, `exists`, `too_long` and `not_editable` show a message. A page built with the website builder (QWeb directives in its body) shows a notice and no form, so it can only be published or unpublished. **Tests:** `test_16_editor_markup_has_no_raw_html_textarea` and `test_09_page_create_edit_publish` in `tests/test_owner_site.py`.

### New page form `GET /my/site/<slug>/page/new` [@ANCHOR: user_websites:COMM_owner_page_new]

**Purpose:** show an empty editor for a new page. **Route:** `owner_page_new(site_slug, **kwargs)`, `auth="user"`, `website=True`, hidden from the sitemap, wrapped by `answers_site_response`. **Behaviour:** `_resolve_site` checks the caller; the template `user_websites.owner_page_edit` gets `page=None`, an empty body, `published=False`, `can_edit_body=True`, and `title` and `page_slug` from the query string if present (used to refill after an error). **Returns:** the rendered form. **Errors:** 404 for a non-owner, 403 explanation pages for a suspended or ineligible owner. Nothing is written until the form is POSTed to `/my/site/<slug>/page/save`. **Test:** `test_09_page_create_edit_publish`, which opens the form and checks the toolbar button `data-uw-cmd="bold"` and `contenteditable`.

### Edit page form `GET /my/site/<slug>/page/<id>` [@ANCHOR: user_websites:COMM_owner_page_edit]

**Purpose:** show the editor for an existing page. **Route:** `owner_page_edit(site_slug, page_id, **kwargs)`, `auth="user"`, `website=True`, hidden from the sitemap. **Behaviour:** `_resolve_site`, then `_own_page` (404 unless the page exists, belongs to this owner or group, and its URL is `/<slug>` or under `/<slug>/`). The body is read back from the stored arch by `_arch_body`; it is sanitized again with `sanitize_member_html` before it reaches the editor, so a hostile stored arch is not shown raw. `can_edit_body` is False for a builder-made page. The address field is the last URL segment, empty for the home page. **Returns:** the rendered form. **Test:** `test_09_page_create_edit_publish`.

### Page save `POST /my/site/<slug>/page/save` [@ANCHOR: user_websites:COMM_owner_page_save]

**Purpose:** create or update a page from the editor form. **Route:** `owner_page_save(site_slug, page_id="", title="", page_slug="", content="", is_published="", **kwargs)`, `auth="user"`, POST only, `csrf=True`. **Steps:** `_resolve_site`; the write rate limit (30 a minute); title trimmed, 1 to 120 characters; body cut-off check (200,000 characters) and `sanitize_member_html`; with `page_id` it must be ASCII digits and an owned, editable page (else 404 or `not_editable`); without it the address defaults to `slugify(title)`, must match `PAGE_SLUG_RE`, avoid `RESERVED_PAGE_SLUGS`, and not exist (`exists`). Hostile input is reported through `_trigger_malicious_arch_violation`. An existing page is written as the caller (so the model's ownership guard and quota apply); a new one is created as the service account. **Returns:** redirect to `/my/site/<slug>/page/<id>?saved=1`; a refused save re-renders the editor with HTTP 400 and the typed text. **Tests:** `test_09_page_create_edit_publish`, `test_10_page_address_rules`.

### Page publish toggle [@ANCHOR: user_websites:COMM_owner_page_publish]

**Purpose:** publish or unpublish one page without deleting it. **Route:** `POST /my/site/<slug>/page/<id>/publish`, `owner_page_publish(site_slug, page_id, publish="", **kwargs)`, `auth="user"`, `csrf=True`. **Arguments:** `publish`, `"1"` to publish, anything else to unpublish; optional `next`, honoured only if it starts with `/my/site/`. **Behaviour:** `_resolve_site`, the write rate limit, `_own_page`, then `website.page.write({"website_published": ...})` in the caller's environment, so the model's ownership check applies. **Returns:** HTTP 303 to `next` or to the dashboard with `?saved=1`. **Errors:** 404 for someone else's page, 429 over the rate limit, 503 if Redis is down. **Test:** `test_09_page_create_edit_publish`.

### Post publish toggle [@ANCHOR: user_websites:COMM_owner_post_publish]

**Purpose:** publish or unpublish a blog post from the dashboard. **Route:** `POST /my/site/<slug>/post/<id>/publish`, `owner_post_publish(site_slug, post_id, publish="", **kwargs)`, `auth="user"`, `csrf=True`. **Behaviour:** `_resolve_site`, the write rate limit, then `UserWebsitesController._get_own_blog_post_for_edit(post_id)` (the same owner or group-member check the post editor uses); the post must also belong to this site's owner or group, else 404. It writes `is_published` (true when `publish == "1"`) in the caller's environment. **Returns:** HTTP 303 to the dashboard with `?saved=1`. The post is kept either way; delete is a separate route. **Errors:** 404, 429, 503. **Test:** `test_18_post_publish_toggle` in `tests/test_owner_site.py`.

### Image upload `POST /my/site/<slug>/image` [@ANCHOR: user_websites:COMM_owner_image_upload]

**Purpose:** store a picture for use in the editor or the dashboard. **Route:** `owner_image_upload(site_slug, ufile=None, json_reply="", **kwargs)`, `auth="user"`, POST only, `csrf=True`, upload rate limit 10 a minute. **Rules:** a file is required (400), not empty (400), at most 2 MB (413), and its type is decided by magic number (`_image_mimetype`): JPEG, PNG, GIF or WebP, else 415; at most 100 images per site (409). **Effect:** as the service account it creates a public `ir.attachment` (`res_model` `res.users` or `user.websites.group`, description `user_websites site image`, name from the slugified file name). **Returns:** with `json_reply` a JSON `{"ok", "error", "url"}` (url `/web/image/<id>`); otherwise a 303 redirect to the dashboard with `?saved=1` or `?error=image`. **Tests:** `test_19_image_upload_rules`, `test_20_image_limit_per_site`.

### Site access check `_resolve_site(site_slug)` [@ANCHOR: user_websites:owner_resolve_site]

**Purpose:** the single access decision of every owner route. **Argument:** `site_slug`. **Returns:** `(profile_user, profile_group)`; one is empty. **Rules:** the slug is resolved from the database as the service account. If it is a user's slug the caller must be that user, otherwise 404 (an administrator who is not the owner also gets 404, because the owner interface edits the owner's own content only). A suspended owner gets the 403 page `owner_site_suspended` that explains and links to the appeal; an owner with eligibility gaps gets `owner_site_ineligible` and the account is reconciled (public content hidden). If it is a group slug the caller must be a member and the group not suspended, else 404. An unknown slug tries `_redirect_legacy_slug` (301 or 308 to the live slug) and then 404. The 403 and redirect are raised as `SiteResponse`, turned into a response by `answers_site_response`. **Tests:** `test_07_dashboard_access` (`test_owner_site_denied_for_other_member`), `test_11_cannot_touch_other_members_content`, `test_23_suspended_owner_is_told_why`.

### Rich text editor `UX_RICH_EDITOR` [@ANCHOR: user_websites:UX_RICH_EDITOR]

**What:** the template `rich_editor` (`views/user_websites_templates.xml`) and the script `static/src/js/rich_editor.js`, a `contenteditable` WYSIWYG with a toolbar (Bold, Italic, Underline, Inline code, Heading, lists, Block quote, Code block, alignment, Insert link, Insert image, Undo, Redo). Members see and format the page and never see markup. **Contract of the template:** the calling template sets `editor_name`, `editor_label`, `editor_html` and `upload_url`; the element carries `data-upload-url` and `data-csrf`. On submit the script copies the HTML into a hidden field. **Security:** the browser side only cleans pasted markup; the server is the authority and re-sanitizes with `sanitize_member_html`. Odoo's own `html_editor` is deliberately not used, because it needs the sitewide designer permission members must not have. **Tests:** `test_16_editor_markup_has_no_raw_html_textarea` (no raw textarea, toolbar and labels present), and the tour `owner_site_tour.js`.

### Member HTML allow-list `member_html_sanitize` [@ANCHOR: user_websites:member_html_sanitize]

**What:** the allow-list constants and the policy of `models/member_html.py`, applied to everything a member writes in the site editor. **Allowed tags:** `p br hr h1-h4 strong b em i u s strike sub sup ul ol li blockquote pre code a img div span table thead tbody tr th td figure figcaption`. **Dropped with their content:** script, style, iframe, object, embed, form controls, link, meta, base, math, audio, video, canvas, dialog and a few more. **Attributes:** no `style`, no `data-*`, no `t-*`; classes only the alignment set `text-start`, `text-center`, `text-end`, `text-justify`; links keep `http`, `https`, `mailto` and same-site paths; images keep only `/web/image/` or `/web/content/` paths so nothing can be hot-linked from a visitor's browser. `MAX_BODY_CHARS` is 200,000. The sanitizer runs in the request path even for administrators. **Test:** `test_script_handlers_and_qweb_are_removed_and_flagged` and `test_toolbar_markup_survives` in `tests/test_owner_site.py`, plus `test_member_html_sanitize_strips_hostile_markup`.

### Standalone member-site layout [@ANCHOR: user_websites:member_site_standalone_layout]

**What:** the first `xpath` of `layout_inherit_user_navbar`, an inheritance of `website.layout` in `views/user_websites_templates.xml`. **Behaviour:** it computes `member_site`, true for the pages of a member or group site (a `website.page`, `blog.blog` or `blog.post` main object, the site's own profile record, or a request path equal to or under `/<slug>`), and for owner tool pages that pass `member_site=True`. When `member_site` is true it sets `no_header` and `no_footer`, which `website.layout` honours, so hams.com's own menu, banners and footer are not shown; the site's own header (`user_navbar`) and footer (`member_site_footer`, class `uw-member-footer`) are. It also sets the context variables `profile_user`, `profile_group`, `resolved_slug`, `page_type` and `is_owner`, and the `user_websites_slug` meta tag. **Tests:** `test_01_member_site_has_no_hams_chrome` (home, blog and a sub-page, as a guest and as the owner) and `test_02_normal_hams_pages_keep_the_hams_chrome`.

### `_image_mimetype(data)` [@ANCHOR: user_websites:owner_image_mimetype]

**Purpose:** decide an uploaded image's type from its first bytes, never from the file name or the declared content type, so a script renamed `.png` is refused. **Argument:** `data`, the bytes read from the upload. **Returns:** `"image/jpeg"` for `FF D8 FF`, `"image/png"` for the eight-byte PNG signature, `"image/gif"` for `GIF87a` or `GIF89a`, `"image/webp"` for `RIFF....WEBP`, otherwise `None`. SVG is not accepted, because it can carry script. No errors, no I/O. The caller maps `None` to HTTP 415 and stores the detected type, not the declared one. **Test:** `test_image_magic_numbers` in `tests/test_owner_site.py` (PNG, JPEG, WebP, an SVG string and a truncated `GIF` give the expected result) and `test_owner_image_upload_rules`.

### `_page_arch(site_slug, page_key, title, body_html)` [@ANCHOR: user_websites:owner_page_arch]

**Purpose:** builds the stored arch of a member page: a fixed frame around already sanitized body markup. **Arguments:** `site_slug` (unused in the text, the site's slug), `page_key` (used in the template name, hyphens replaced by underscores), `title` (escaped with `_escape_attr`), `body_html` (output of `sanitize_member_html`, may be empty). **Returns:** the XML string `<t name="..." t-name="user_websites.page_<key>"><t t-call="user_websites.template_default_home"><div id="wrap" class="oe_structure uw-member-page">...</div></t></t>`. The body is parsed with lxml and re-serialized as XML, so it is well-formed; top-level text is escaped. The result contains exactly the two QWeb directives that the model sanitizer permits. No errors apart from lxml parse failures on non-well-formed input, which the sanitizer prevents. **Test:** `test_09_page_create_edit_publish`.

### `_arch_body(arch_base)` [@ANCHOR: user_websites:owner_page_arch_body]

**Purpose:** reads the editable body back out of a stored arch, the inverse of `_page_arch`. **Argument:** `arch_base`, the stored arch string or bytes (may be empty). **Returns:** the tuple `(body_html, editable)`. An empty arch gives `("", True)`; XML that does not parse, an arch without `<div id="wrap">`, or a wrap holding any `<t>` element or `t-*` attribute (a page built by the website builder) gives `("", False)`, so the editor is not offered. Otherwise the inner HTML of the wrap, with top-level text escaped, and True. **Effects:** none. The editor route still sanitizes the result before showing it. **Test:** `test_09_page_create_edit_publish` (edit and re-save keeps the body) and `test_16_editor_markup_has_no_raw_html_textarea`.

### `_is_ascii_digits(value)` [@ANCHOR: user_websites:COMM_owner_ascii_digits]

**Purpose:** stops a non-numeric page id in a posted form from becoming an HTTP 500. **Argument:** `value`, anything (converted with `str`). **Returns:** `True` only for a non-empty string of ASCII digits, exactly what `int()` accepts. `str.isdigit()` alone is true for characters such as the superscript two, which `int()` refuses and which once made `/page/save` answer 500. **Callers:** `owner_page_save`, `_editor_with_error` and `_back_to_editor` check it before `int(page_id)`; a failure gives `request.not_found()` (404). No errors, no effects. **Test:** `test_10b_a_page_id_that_is_not_ascii_digits_is_a_404_not_a_500` in `tests/test_owner_site.py`.

### `RichEditor.start()` [@ANCHOR: user_websites:rich_editor_start]

**Purpose:** wires one member-site editor instance. `RichEditor` is a public interaction in `static/src/js/rich_editor.js` bound to each `.uw-rich-editor` element, and Odoo calls `start()` once per element. **Arguments:** none; it reads `this.el`. **Behaviour:** finds the editable area (`[data-uw-area]`), the hidden field (`[data-uw-field]`), the status line, the link bar, link input and file input, records the area's HTML as the first undo step, copies it into the field, and registers listeners for toolbar click and mousedown (the mousedown default is prevented so the page selection survives a button press), paste, input, keydown (Ctrl or Cmd Z and Y), beforeinput, keyup, mouseup, the link bar, the link input's keydown, the file input's change, and the enclosing form's submit. **Returns:** nothing. **Errors:** none handled; a missing element would throw at load. No network. **Test:** the tour `owner_site_tour` (`test_tour_owner_site`).

### `RichEditor.syncField()` [@ANCHOR: user_websites:rich_editor_sync_field]

**Purpose:** keeps the hidden form field equal to what the member sees, so the server receives the editor's HTML when the form is submitted. **Arguments:** none. **Behaviour:** assigns `this.area.innerHTML` to `this.field.value`. It is called by `start()`, after every input, toolbar action, paste, link and image insertion, on undo and redo, and by the form's `submit` listener as a last step. **Returns:** nothing. **Errors:** none. **Security:** the value is untrusted input to the server, which re-sanitizes it with `sanitize_member_html` and refuses bodies over 200,000 characters. No network. **Test:** the tour `owner_site_tour`, which saves a post and then checks the stored content contains the bold text.

### `RichEditor.say(message)` [@ANCHOR: user_websites:rich_editor_say]

**Purpose:** speaks a short status to the member, including screen-reader users. **Argument:** `message`, a string; an empty or missing value clears the status. **Behaviour:** sets `textContent` of the element `[data-uw-status]` (the editor's status line, a live region in the template), so the text is never interpreted as markup. **Returns:** nothing. **Used for:** "Links must start with http://, https://, mailto: or /.", "Images can be at most 2 MB.", "Uploading image...", the upload error text returned by the server, and "Image added. Describe it in the page text for readers who cannot see it." **Errors and effects:** none besides the DOM text. **Test:** the tour `owner_site_tour` exercises the editor in which the line lives.

### `RichEditor.onToolbarClick(ev)` [@ANCHOR: user_websites:rich_editor_toolbar_click]

**Purpose:** runs a toolbar button. **Argument:** `ev`, the click event on the toolbar; it finds the closest `button[data-uw-cmd]` and ignores disabled buttons and other clicks. **Behaviour:** focuses the area, then by command: `link` opens the link bar, `image` remembers the selection and opens the file picker, `code` calls `wrapSelectionInCode`, `undo` and `redo` step the editor's own history, `formatBlock` runs `document.execCommand("formatBlock", false, data-uw-arg)`, and any other command (bold, italic, underline, lists, alignment) runs `execCommand(cmd)`. Except for link, image, undo and redo it then records an undo step. It finishes with `syncField()` and `refreshToolbarState()`. **Returns:** nothing. **Errors:** none caught. **Test:** the tour `owner_site_tour` (bold, undo, redo).

### `RichEditor.refreshToolbarState()` [@ANCHOR: user_websites:rich_editor_toolbar_state]

**Purpose:** shows which formats are active at the caret. **Arguments:** none. **Behaviour:** for every toolbar button whose command is in `TOGGLE_STATE` (bold, italic, underline, the two list commands, three alignments) it asks `document.queryCommandState`, treating an exception as off, and sets `aria-pressed="true"` or `"false"` and toggles the `active` class. Other buttons are not touched. It runs after each toolbar action and on keyup and mouseup in the area. **Returns:** nothing. **Errors:** swallowed (`false`). **Accessibility:** `aria-pressed` is what lets a screen reader announce a toggle. No network. **Test:** the tour `owner_site_tour`, which presses Bold and reads the result.

### `RichEditor.rememberRange()` [@ANCHOR: user_websites:rich_editor_remember_range]

**Purpose:** saves the caret or selection before focus leaves the page text, for example when the link bar input or the file picker takes focus. **Arguments:** none. **Behaviour:** if the window selection has a range whose anchor lies inside the editable area, a clone of the first range is stored in `this.savedRange`; otherwise the old value is kept. **Returns:** nothing. **Errors:** none. **Used by:** `openLinkBar` and the image command of `onToolbarClick`; `restoreRange` puts it back. No network, no storage beyond the instance field. **Test:** the tour `owner_site_tour` through the link and toolbar paths; no step inspects the saved range directly (unknown, to-do: assert the restored selection after inserting a link).

### `RichEditor.restoreRange()` [@ANCHOR: user_websites:rich_editor_restore_range]

**Purpose:** puts the saved selection back so a command that needs it (link, image) applies where the member was working. **Arguments:** none. **Behaviour:** focuses the editable area; if `this.savedRange` is set it clears the window selection and adds that range. **Returns:** nothing. **Errors:** none. **Used by:** `closeLinkBar` (cancel or after applying a link) and `onImagePicked` (before `insertHTML` of the uploaded image), so the image appears at the caret and not at the start of the page. Calling it with no saved range only focuses the area. No network. **Test:** the tour `owner_site_tour`.

### `RichEditor.wrapSelectionInCode()` [@ANCHOR: user_websites:rich_editor_wrap_code]

**Purpose:** the "Inline code" button. **Arguments:** none; it uses the window selection. **Behaviour:** if there is no range or the selection is collapsed it does nothing. Otherwise it takes the selected text, creates a `<code>` element whose `textContent` is that text (so markup inside the selection is flattened and nothing is injected), deletes the selected contents and inserts the element at the range. **Returns:** nothing. **Errors:** none. The caller `onToolbarClick` then records an undo step and syncs the hidden field. The server allow-list keeps `code`, so the result survives saving. **Test:** the tour `owner_site_tour` cites it with the toolbar commands; no step presses the code button (unknown, to-do).

### `RichEditor.openLinkBar()` [@ANCHOR: user_websites:rich_editor_open_link_bar]

**Purpose:** shows the small link entry bar when the member presses "Insert link". **Arguments:** none. **Behaviour:** calls `rememberRange()`, removes the `d-none` class from the link bar, sets the input to `https://` and focuses it. Nothing is applied until Enter or the Apply button. **Returns:** nothing. **Errors:** none. **Why a bar:** `window.prompt` is not accessible and blocks the page, so the editor has its own input. The result is validated by `applyLink` in the browser and again by `sanitize_member_html` on the server (only `http`, `https`, `mailto` and same-site paths survive). No network. **Test:** the tour `owner_site_tour`.

### `RichEditor.closeLinkBar()` [@ANCHOR: user_websites:rich_editor_close_link_bar]

**Purpose:** hides the link bar. **Arguments:** none. **Behaviour:** adds `d-none` to the link bar and calls `restoreRange()` so focus and selection return to the page text. It is called by Cancel, by Escape in the input, and by `applyLink` before it validates. **Returns:** nothing. **Errors:** none. It does not clear the input; `openLinkBar` resets it to `https://` the next time. No network. **Test:** the tour `owner_site_tour` cites it with the other link-bar functions; the tour does not exercise Cancel (unknown, to-do).

### `RichEditor.onLinkBarClick(ev)` [@ANCHOR: user_websites:rich_editor_link_bar_click]

**Purpose:** handles clicks inside the link bar. **Argument:** `ev`, the click event. **Behaviour:** a click on an element inside `[data-uw-link-apply]` calls `applyLink()`; inside `[data-uw-link-cancel]` calls `closeLinkBar()`; any other click does nothing. The listener is registered on the bar in `start()`. **Returns:** nothing. **Errors:** none. No network. The Enter and Escape keys are handled by `onLinkKeydown`, so the bar can be used without a mouse. **Test:** the tour `owner_site_tour` (cited with the link-bar functions).

### `RichEditor.onLinkKeydown(ev)` [@ANCHOR: user_websites:rich_editor_link_keydown]

**Purpose:** keyboard control of the link input. **Argument:** `ev`, the `keydown` event of the link input. **Behaviour:** Enter prevents the default (which would submit the page form) and calls `applyLink()`; Escape prevents the default and calls `closeLinkBar()`; other keys are left alone. **Returns:** nothing. **Errors:** none. Preventing Enter matters because the input sits inside the editor's own `<form>`, where Enter would otherwise save the whole page. No network. **Test:** the tour `owner_site_tour` (cited with the link-bar functions); no step presses Enter or Escape in the bar (unknown, to-do).

### `RichEditor.applyLink()` [@ANCHOR: user_websites:rich_editor_apply_link]

**Purpose:** turns the selected text into a link. **Arguments:** none; it reads the link input. **Behaviour:** trims the entered address, closes the bar (restoring the selection), and accepts only addresses starting with `http://`, `https://`, `mailto:` or `/` (case-insensitive); otherwise it says "Links must start with http://, https://, mailto: or /." through `say` and stops. On success it clears the status, runs `document.execCommand("createLink", false, url)`, records an undo step and syncs the hidden field. **Returns:** nothing. **Security:** a `javascript:` or `data:` address is refused here and would be dropped by the server anyway; links saved get `rel="nofollow noopener ugc"` from the sanitizer. No network. **Test:** the tour `owner_site_tour`.

### `RichEditor.onImagePicked()` [@ANCHOR: user_websites:rich_editor_image_picked]

**Purpose:** uploads the image the member chose and inserts it. **Arguments:** none; it reads the file input and resets its value. **Behaviour:** with no file it returns; a file over 2 MB is refused with a message; otherwise it POSTs a `FormData` (`ufile`, `json_reply=1`, `csrf_token` from `data-csrf`) to the `data-upload-url` (`/my/site/<slug>/image`) with same-origin credentials, shows "Uploading image...", and reads the JSON `{ok, error, url}`. A network failure or `ok` false shows the error text. On success it restores the saved range and inserts `<img src="<url>" alt="<file name without extension>"/>` (quotes and angle brackets removed from the alt text), records an undo step, syncs the field and asks the member to describe the image in the text. **Returns:** a promise. **Errors:** reported through `say`. **Test:** the tour `owner_site_tour`; the server limits are tested by `test_19_image_upload_rules`.

### `RichEditor.onPaste(ev)` [@ANCHOR: user_websites:rich_editor_paste]

**Purpose:** keeps pasted content from bringing in styles, scripts and foreign markup. **Argument:** `ev`, the paste event. **Behaviour:** with no `clipboardData` it does nothing (the browser pastes normally). Otherwise it cancels the default; if the clipboard has `text/html` it parses it with `DOMParser`, passes the body through `cleanNode` and inserts the result with `execCommand("insertHTML")`; else it inserts the plain text with `insertText`. Then it records an undo step and syncs the field. **Returns:** nothing. **Errors:** none caught. **Security:** this is a convenience, not the boundary; the server sanitizes again. No network. **Test:** the tour `owner_site_tour` cites it; no step pastes (unknown, to-do: a hoot unit test of `cleanNode` and the paste handler).

### `RichEditor.cleanNode(node)` [@ANCHOR: user_websites:rich_editor_clean_node]

**Purpose:** reduces a pasted DOM tree to the structure the editor supports. **Argument:** `node`, a DOM node (usually the parsed clipboard body). **Returns:** an HTML string. **Rules:** text nodes are escaped for `& < >`; elements in `DROP_TAGS` (script, style, iframe, object, embed, form controls, link, meta, svg, math, media and similar) are removed with their content; `A` keeps only an `href` starting with `http://`, `https://`, `mailto:` or `/` (quotes escaped), otherwise only its text; `IMG` is dropped (pasted images would hot-link), `BR` and `HR` become self-closing tags; other block and inline tags in the editor's lists are re-emitted without attributes, except `SPAN` and `DIV`, which are unwrapped to their content; any unknown tag is unwrapped. It recurses over child nodes. No errors, no DOM changes, no network. **Test:** the tour `owner_site_tour` cites it; server-side equivalents are tested by `test_external_images_and_foreign_classes_are_dropped`.

### Owner site tour `test_tour_owner_site` [@ANCHOR: test_tour_owner_site]

The browser tour `owner_site_tour` in `static/tests/tours/owner_site_tour.js`, run by `test_12_owner_writes_and_publishes_a_post` in `tests/test_ui_tours.py` as the fixture member `touruser`. Steps: the owner toolbar is present, "New blog post" is pressed, the contenteditable editor appears, a title is typed, text is inserted, Bold is pressed, Undo removes the bold and does not reach into the title, Redo twice restores text and bold, "Published" is ticked, and the form is saved. The Python test then asserts exactly one post owned by the member, published, named "Tour Post Title", whose content contains `<b>Written without markup`. It is the single end-to-end check cited by every `rich_editor_*` anchor. Needs a headless browser.

### `_serve_core_fallback()` [@ANCHOR: user_websites:fn_main_serve_core_fallback]

**Purpose:** gives Odoo's own page, redirect and attachment lookup a chance for a path that is not a member site. **Arguments:** none; it reads the current `request`. **Behaviour:** removes `website_slug` from `request.params` and returns `request.env["ir.http"]._serve_fallback()`. The removal matters: the `/<website_slug>` route puts the slug into the request parameters, and `website.page._allow_to_use_cache()` refuses to cache a response whose request has parameters, so every stock page would lose its edge caching. **Returns:** a response, or `None` when nothing matches, in which case the caller raises 404. **Used by:** `user_home_fallback` and `user_home_legacy_redirect`. No locks; read-only lookups. **Test:** `test_helper_functions_are_exercised` and `test_05_non_member_single_segment_paths_still_served` in `tests/test_member_site_home_url.py`.

### `_escape(text)` [@ANCHOR: user_websites:fn_owner_escape]

**Purpose:** escapes text that goes into an XML element body when a page arch is built. **Argument:** `text`, a string. **Returns:** the string with `&` replaced by `&amp;`, `<` by `&lt;` and `>` by `&gt;`, in that order so existing entities are not double-escaped in the wrong direction. It does not escape quotes, so use `_escape_attr` inside an attribute. **Used by:** `_page_arch` (top-level body text) and `_arch_body` (when reading text back out of a wrap). No errors, no effects. **Test:** `test_helper_functions_are_exercised` and, through arch building and re-reading, `test_09_page_create_edit_publish` in `tests/test_owner_site.py`.

### `_escape_attr(text)` [@ANCHOR: user_websites:fn_owner_escape_attr]

**Purpose:** escapes text that goes into a double-quoted XML attribute, used for the page title in `<t name="...">`. **Argument:** `text`, a string. **Returns:** `_escape(text)` with `"` additionally replaced by `&quot;`, so a title such as `a "quoted" <b>` cannot end the attribute or open an element in the stored arch. No errors, no effects. The member's title is also limited to 120 characters by the save route, and the model sanitizer runs on the arch for non-administrators. **Test:** `test_helper_functions_are_exercised` and `test_09_page_create_edit_publish` in `tests/test_owner_site.py`.

### `UserWebsitesOwnerController._svc()` [@ANCHOR: user_websites:fn_owner_svc]

**Purpose:** the owner interface's single door to the service account. **Arguments:** none. **Returns:** `request.env["zero_sudo.security.utils"]._get_service_env("user_websites.user_websites_service_account")`, an environment for the User Websites service account (never `sudo()`). **Use:** reads of other members' tables that the signed-in member has no rights to (slug resolution, listing the owner's pages, posts, images) and creates of pages and attachments after the ownership check; edits of existing pages and posts run in the caller's own environment so the model's ownership guard, field allow-list and quotas stay in force. **Errors:** `_get_service_env` raises if the account is not provisioned. No side effects of its own. **Test:** `test_helper_functions_are_exercised`; every owner route test passes through it.

### `_owner_domain(profile_user, profile_group)` [@ANCHOR: user_websites:fn_owner_owner_domain]

**Purpose:** the search domain that selects one site's own records. **Arguments:** `profile_user` (a `res.users` record or empty) and `profile_group` (a `user.websites.group` record or empty), exactly one of which is set by `_resolve_site`. **Returns:** `[("owner_user_id", "=", profile_user.id)]` for a user site, else `[("user_websites_group_id", "=", profile_group.id)]`. The dashboard adds it to its searches of `website.page` and `blog.post`, including the deleted-items lists. No errors, no database access of its own, pure. If both arguments were empty the group branch would raise on `.id` of an empty record, but `_resolve_site` never returns that. **Test:** `test_helper_functions_are_exercised` and `test_08_dashboard_lists_content`.

### `_qcontext(profile_user, profile_group, **extra)` [@ANCHOR: user_websites:fn_owner_qcontext]

**Purpose:** builds the QWeb context shared by every owner page so the member-site layout renders correctly. **Arguments:** the two profile records (as above) and keyword arguments of the page's own values. **Returns:** a dict with `profile_user`, `profile_group`, `main_object` (the user or the group), `resolved_slug` and `site_slug` (that record's `website_slug`), `member_site` True (which selects the standalone member layout), `is_owner` True, `error` and `saved` (copied from the query string for the notice boxes), then updated with `extra`, which can override any key. No errors, no side effects, no writes. The owner interface is only reachable after `_resolve_site`, which is why `is_owner` can be hard-coded. **Test:** `test_helper_functions_are_exercised`; the dashboard and editor tests render with it.

### `_report_if_hostile(hostile, profile_user, profile_group, url)` [@ANCHOR: user_websites:fn_owner_report_if_hostile]

**Purpose:** files the automatic violation report when the editor's sanitizer found something only a hand-made request carries. **Arguments:** `hostile` (the flag from `sanitize_member_html`), the two profile records, and `url` (the page address). **Behaviour:** with `hostile` false it returns at once. Otherwise it builds `{"url": url, "owner_user_id": ...}` or `{"url": url, "user_websites_group_id": ...}` and calls `website.page._trigger_malicious_arch_violation(vals)`, which creates or reuses a report for that URL and strikes the owner (strikes are not doubled for an already resolved report). **Returns:** `None`. **Side effects:** a database write and a strike; three strikes suspend the account. **Test:** `test_13_hostile_body_is_sanitized_and_reported` and `test_14_internal_owner_body_is_sanitized_too` in `tests/test_owner_site.py`.

### `_clean_body(content)` [@ANCHOR: user_websites:fn_owner_clean_body]

**Purpose:** the owner routes' one call for turning posted editor HTML into stored body text. **Argument:** `content`, the posted string (may be `None`). **Returns:** `(clean_html, hostile)`. Over `MAX_BODY_CHARS` (200,000 characters) it returns `(None, False)`, which `owner_page_save` answers with the `too_long` error; otherwise it returns the result of `sanitize_member_html(content)`. It runs for every caller, administrators and the owner's own account included, because the model sanitizers skip administrators and the service account. No errors of its own, no writes. **Tests:** `test_helper_functions_are_exercised` and `test_13_hostile_body_is_sanitized_and_reported` in `tests/test_owner_site.py`. Unknown: no test posts a body over the limit to see `too_long`; to-do.

### `_own_page(profile_user, profile_group, page_id)` [@ANCHOR: user_websites:fn_owner_own_page]

**Purpose:** finds one active page the caller may edit, or ends with 404. **Arguments:** the two profile records and `page_id` (an `int`). **Returns:** the `website.page` record (read as the service account). **Errors:** `request.not_found()` (HTTP 404) when the page does not exist, when its owner user or group differs from the site's, or when its URL is neither `/<slug>` nor under `/<slug>/`, so another member's page id and a page outside the owner's namespace look the same as a missing page. Archived pages are not found (see `_own_page_any`). No writes. **Callers:** the edit, save, publish, delete and confirm routes. **Test:** `test_11_cannot_touch_other_members_content`, `test_21_page_urls_stay_in_the_owners_namespace` in `tests/test_owner_site.py`.

### `_back_to_editor(slug, page_id, error)` [@ANCHOR: user_websites:fn_owner_back_to_editor]

**Purpose:** sends the member back to the editor with an error code, for failures that do not keep the typed text. **Arguments:** `slug` (the site's slug, URL-quoted with nothing safe), `page_id` (the posted id string) and `error` (a fixed code such as `not_editable`). **Returns:** a redirect to `/my/site/<slug>/page/<id>?error=<code>`, or to `/page/new` when `page_id` is not ASCII digits. It does not use the id in the URL unless `_is_ascii_digits` accepted it, so a crafted id cannot add path segments. No errors, no writes. Used when a page built with the website builder cannot be edited in this editor. **Test:** `test_helper_functions_are_exercised`. Unknown: no test posts to a builder-made page to see `not_editable`; to-do.

### `_attachment_owner(profile_user, profile_group)` [@ANCHOR: user_websites:fn_owner_attachment_owner]

**Purpose:** tells which record an uploaded image belongs to. **Arguments:** the two profile records. **Returns:** `("res.users", profile_user.id)` for a user site, else `("user.websites.group", profile_group.id)`. The pair becomes `res_model` and `res_id` of the `ir.attachment`, together with the description `user_websites site image`; the dashboard, the 100-image limit and the GDPR erasure (for users) all select on it. No errors, no database access, pure. Group images are not removed by a member's account erasure because they belong to the group. **Test:** `test_helper_functions_are_exercised`, `test_19_image_upload_rules` (the attachment's `res_model` and `res_id`) and `test_20_image_limit_per_site`.

### `_clean_url(value, image=False)` [@ANCHOR: user_websites:fn_hmtl_clean_url]

**Purpose:** validates one `href` or `src` the member's HTML carries. **Arguments:** `value` (string or `None`) and `image` (True for an image source). **Returns:** the trimmed value if it is allowed, else `None`. **Rules:** an empty value or any control character gives `None`; for an image only paths of the form `/web/image/<id>...` or `/web/content/<id>...` (`IMAGE_PATH_RE`) pass, so an external tracking pixel is refused; for a link, `#fragment`, a same-site path starting with a single `/` (not `//`), and the schemes `http`, `https` and `mailto` pass; everything else (`javascript:`, `data:`, `//host`, relative paths) is `None`. No errors, no side effects. **Test:** `test_helper_functions_are_exercised`, `test_external_images_and_foreign_classes_are_dropped` and `test_toolbar_markup_survives`.

### `_is_hostile(el)` [@ANCHOR: user_websites:fn_hmtl_is_hostile]

**Purpose:** decides whether an element is something only a hand-made request could carry, to flag the save as an attack attempt (a strike) rather than a clumsy paste. **Argument:** `el`, an lxml element. **Returns:** True for a tag in `DROP_WITH_CONTENT` (script, style, iframe and the rest), for any attribute beginning with `on` (event handlers), `t-` (QWeb) or `xlink`, and for an `href`, `src`, `action` or `formaction` beginning with `javascript:`, `vbscript:` or `data:` (except `data:image/`). Otherwise False. Comments and processing instructions have no string tag and are not hostile. No errors, no changes to the tree. Cleaning is done separately by `_clean_children`. **Test:** `test_script_handlers_and_qweb_are_removed_and_flagged` in `tests/test_owner_site.py`.

### `sanitize_member_html(source)` [@ANCHOR: user_websites:fn_hmtl_sanitize_member_html]

**Purpose:** the sanitizer for everything members write in the site editor. **Argument:** `source`, a string or `None`. **Returns:** `(clean_html, hostile)`: a well-formed HTML fragment with only allow-listed markup, and True when something the toolbar cannot produce was found. Blank input or a parse failure gives `("", False)`. **Algorithm:** input is cut to twice `MAX_BODY_CHARS`; it is parsed as a fragment inside a `<div>`; `_is_hostile` is run on every element to set the flag; `_clean_children` rewrites the tree (drops forbidden elements with content, unwraps unknown tags keeping their text, filters attributes, handles `svg` through the SVG allow-list, adds `rel="nofollow noopener ugc"` to links); the wrapper is removed and the result stripped. No I/O, no locks. The caller reports hostile input as a content violation. **Test:** `test_script_handlers_and_qweb_are_removed_and_flagged`, `test_toolbar_markup_survives`, `test_member_html_sanitize_strips_hostile_markup`.

### `_clean_children(parent)` [@ANCHOR: user_websites:fn_hmtl_clean_children]

**Purpose:** the recursive cleaning pass of the sanitizer. **Argument:** `parent`, an lxml element; its children are changed in place. **Returns:** `None`. **Per child:** comments and processing instructions are dropped; tags in `DROP_WITH_CONTENT` are removed with their content; `svg` is replaced by the allow-listed result (`_replace_with_sanitized_svg`); otherwise it recurses first, then, if the tag is not in `ALLOWED_TAGS`, the element is unwrapped (its text and children stay); allowed tags have their attributes rebuilt by `_clean_attributes`; an `a` without `href` is unwrapped, an `img` without `src` is dropped, and every remaining `a` gets `rel="nofollow noopener ugc"`. No errors raised; no I/O. Recursion depth follows the document depth, which is limited by the body size cap. **Test:** `test_helper_functions_are_exercised` and the sanitizer tests in `tests/test_owner_site.py`.

### `_clean_attributes(el, tag)` [@ANCHOR: user_websites:fn_hmtl_clean_attributes]

**Purpose:** rebuilds the attributes of one allowed element from scratch, so nothing is kept by accident. **Arguments:** `el` (lxml element) and `tag` (its lower-case name). **Behaviour:** reads the original attributes, keeps only classes in `ALLOWED_CLASSES` (`text-start`, `text-center`, `text-end`, `text-justify`), converts a `text-align:` value in a `style` attribute to the matching class, then removes every attribute and sets back only: the class list; for `a` a `href` that passed `_clean_url` and a `title` cut to 300 characters; for `img` a `src` that passed the image check, an `alt` cut to 300 characters and numeric `width` and `height`; for `td` and `th` numeric `colspan` and `rowspan`. So no `style`, `data-*`, `id`, `on*` or `t-*` attribute survives. Returns `None`; changes the element in place. **Test:** `test_toolbar_markup_survives` (the `text-align:center` becomes `text-center`, no `style=`).

### `_drop(el)` [@ANCHOR: user_websites:fn_hmtl_drop]

**Purpose:** removes an element with all its content but keeps the text that followed it. **Argument:** `el`, an lxml element. **Behaviour:** if it has no parent nothing happens. Otherwise its `tail` text is appended to the previous sibling's tail, or to the parent's text when it is the first child, and the element is removed from the parent. Without this step lxml would delete the text after a removed `<script>`, losing the member's words. **Returns:** `None`. **Errors:** none. Used by `_clean_children` for forbidden tags, comments and an image without a source. **Test:** `test_helper_functions_are_exercised` and `test_script_handlers_and_qweb_are_removed_and_flagged`, which asserts that the visible text `Hi` survives the removal of the markup around it.

### `_unwrap(el)` [@ANCHOR: user_websites:fn_hmtl_unwrap]

**Purpose:** removes an element but keeps its text and children, for tags that are not allowed (a stray `font`, `center`, an `a` with no address). **Argument:** `el`, an lxml element. **Behaviour:** without a parent it does nothing. Otherwise the element's own text goes to the previous sibling's tail (or the parent's text), its children are inserted in its place in order, the element is removed, and its tail is attached after the last moved child (or after the previous sibling, or the parent's text). The visible content and its order are unchanged. **Returns:** `None`. **Errors:** none. Used by `_clean_children`. **Test:** `test_helper_functions_are_exercised` and `test_script_handlers_and_qweb_are_removed_and_flagged`.

### `menu_pages(env, domain_field, record_id, slug)` [@ANCHOR: user_websites:fn_hmtl_menu_pages]

**Purpose:** the extra links in a member site's navbar. **Arguments:** `env` (an Odoo environment), `domain_field` (`"owner_user_id"` or `"user_websites_group_id"`), `record_id` (the user or group id) and `slug` (the site slug). **Returns:** a list of up to 8 dicts `{"name", "url"}`: the published `website.page` records of that owner whose URL is `/<slug>/<one segment>` (no deeper), ordered by name then id, skipping any whose last segment is in `RESERVED_PAGE_SLUGS`. They are read as the User Websites service account, because a visitor cannot read pages by owner. No writes, no errors of its own. The navbar template `user_navbar` calls `_user_websites_menu_pages()`, which wraps this. **Test:** `test_helper_functions_are_exercised` and, through the navbar, `test_01_member_site_has_no_hams_chrome`; no test asserts the menu entries themselves (unknown, to-do).

### `res.users._user_websites_menu_pages()` [@ANCHOR: user_websites:fn_users_user_websites_menu_pages]

**Purpose:** the menu pages of a member's site, as a method of the user. **Arguments:** none; `self` must be one record (`ensure_one`). **Returns:** `menu_pages(self.env, "owner_user_id", self.id, self.website_slug)` when the user has a `website_slug`, else an empty list. The result is a list of up to 8 `{"name", "url"}` dicts for the navbar. Read-only; the pages are read as the service account inside `menu_pages`. It is called from `user_websites.user_navbar` with `profile_user`, and the group model has the same method name so the template does not care which kind of site it renders. **Errors:** `ensure_one` raises `ValueError` for an empty or multi-record set. **Test:** `test_helper_functions_are_exercised` in `tests/test_owner_site.py`.

### `user.websites.group._user_websites_menu_pages()` [@ANCHOR: user_websites:fn_group_user_websites_menu_pages]

**Purpose:** the menu pages of a group site's navbar. **Arguments:** none; `self` must be one record. **Returns:** `menu_pages(self.env, "user_websites_group_id", self.id, self.website_slug)` when the group has a slug, else `[]`: published pages one level under `/<slug>`, up to 8, ordered by name, without reserved page words. It mirrors `res.users._user_websites_menu_pages` so `user_navbar` can call `(profile_user or profile_group)._user_websites_menu_pages()`. Read-only, no locks; the pages are read as the service account. **Errors:** `ensure_one` raises `ValueError` otherwise. **Test:** `test_helper_functions_are_exercised` in `tests/test_owner_site.py` cites it (it does not call the group method directly: unknown, to-do).

### `_replace_with_sanitized_svg(el)` [@ANCHOR: user_websites:member_html_svg_allowlist]

**Purpose:** keeps an SVG schematic a member pasted or imported, while removing everything active. **Argument:** `el`, an lxml `svg` element inside the tree being cleaned. **Behaviour:** serializes the element, passes it through Odoo's `html_sanitize` with zero_sudo's SVG allow-list (shapes and text kept; script, event handlers, links, `<use>`, `data:` URLs and foreign objects removed), parses the result and inserts the resulting nodes where the element was, then removes the original and keeps the original tail text. If the result cannot be parsed the element is simply removed. **Returns:** `None`. The editor cannot draw SVG; this path only keeps earlier content. A hostile SVG still sets the hostile flag through `_is_hostile` only for the dangerous attributes it carries. **Test:** `test_svg_schematic_is_kept_and_hostile_svg_is_stripped` (a clean SVG stays, an `onload` and `<script>` are gone and flagged) and `test_svg_allowlist_sanitizer_fields`.
