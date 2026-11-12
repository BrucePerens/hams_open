# User Websites (`user_websites`)

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).*

Welcome to User Websites! This Odoo 19 module allows you to build and manage your own personal or group websites and blogs directly within Odoo. Whether you're a beginner or an expert, you'll find everything you need to create a professional online presence.

**Open Source Rule:** We built this for the open-source community. It runs perfectly on its own and does not rely on any proprietary code.

## 🌟 What Can I Do?

*   **Create Your Personal Site:** Get a unique URL (like `/yourname`) and use our easy drag-and-drop editor to build your pages.
*   **Start a Blog:** Share your thoughts and stories with the community. Everyone gets their own blog section.
*   **Collaborate with Groups:** Create shared websites for your team, club, or project.
*   **Join the Community:** Choose to show off your site in our public directory and discover what others are building.
*   **Control What Is Public:** You decide which of your pages are published. There is no expectation of privacy for anything you publish or enter. We also provide GDPR data export and content-erasure tools, which exist because the law requires them.

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

* **Tenant Isolation:** Enforced via strict record rules verified by `test_tenant_view_isolation` and ACL overhead elimination to prevent log spam `test_acl_overhead_loop_elimination`.
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
* **`content.violation.report`** (model itself now in `content_moderation`, this module's own extension in `models/content_violation_report_moderation.py`): Stores abuse reports. Originator is masked from the target owner. The system automatically generates a report and issues a strike if a user attempts to inject malicious SSTI/XSS payloads into their site architecture `content_moderation:action_take_action_and_strike` (via this module's own enforcement-hook override `user_websites:COMM_apply_enforcement_action`), tested by `test_moderation_suspension`. Admin spam is prevented via a daily digest cron (`ir_cron_notify_pending_reports` `ir_cron_notify_pending_reports`, `cron_notify_pending_reports`, verified by `test_cron_pending_reports`) and a UI toast that is sticky but remembered in localStorage for 12 h per (count, newest id), with Review/Dismiss buttons, (`[@ANCHOR: toast_notifications_logic]`, `[@ANCHOR: admin_toast_logic]`, tested by `[@ANCHOR: test_tour_toast_notifications]`).

* **Security Auto-Moderation**: The `website.page` model includes `_sanitize_user_arch` `website_page_sanitize_arch`, verified by `test_website_page_sanitize_arch`, which forcefully strips dangerous QWeb directives (`t-*`) and JS event handlers.
* **Member CSS**: `<link>` is removed; `<style>` is rebuilt rule by rule and every `style="..."` attribute declaration by declaration with zero_sudo's `css_sanitizer` `user_websites:page_arch_style_attribute_filter`, verified by `test_user_arch_style_attribute_filter`: no `url()` except `#fragment`, no `image-set()`, `data:`, `expression`, escapes, and `position` only static, relative or absolute. A refused declaration is dropped (no strike); malformed CSS drops the whole attribute. Blog post content goes through `html_sanitize` instead and is not covered by this filter.
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

* **Violation Reporting:** Form endpoint `/website/report_violation` `[@ANCHOR: user_websites:UX_REPORT_VIOLATION]`, `[@ANCHOR: violation_report_logic]`, verified by `[@ANCHOR: test_tour_violation_report]`. Protected by the `form_shield` module (proof of work or question, honeypots, minimum time-to-submit, per-network, per-page and per-site limits `[@ANCHOR: user_websites:report_limits]`, `[@ANCHOR: user_websites:report_violation_shield]`, the form's fields `[@ANCHOR: user_websites:report_violation_shield_fields]`); the reported URL must be a page of this site (this host, the base URL or a website domain, http and https alike, www ignored `[@ANCHOR: user_websites:report_own_site_hosts]`) `[@ANCHOR: user_websites:normalize_report_target]` and a guest's report is refused when it is short, one random-looking string, link-heavy or lacks a valid address `[@ANCHOR: user_websites:report_problem]`. `GET /website/report_violation` is the question variant for browsers without scripts `[@ANCHOR: user_websites:report_violation_page]`. Helpers: the redirect path is forced to a single leading slash `[@ANCHOR: user_websites:reported_path]`, hosts compare without `www.` `[@ANCHOR: user_websites:bare_host]`, a guest's address may not start or end its local part with a dot `[@ANCHOR: user_websites:local_part_ok]`, the question page is rendered by `[@ANCHOR: user_websites:render_report_page]` and a refusal is answered by `[@ANCHOR: user_websites:report_refusal]`; the old per-address redirect-path anchor `[@ANCHOR: user_websites:report_violation_redirect_target]` stays.

* **Home Routing:** Target view `/<slug>` (the old `/<slug>/home` answers a permanent 301 to it, `[@ANCHOR: user_websites:COMM_user_home_legacy_redirect]`) `[@ANCHOR: controller_user_websites_home]`.

* **Site Creation:** `/<slug>/create_site` `[@ANCHOR: UX_CREATE_SITE]`, concurrency scaling proven by `test_site_creation_performance_scaling`.

* **Blog Routing:** `/<slug>/blog` `[@ANCHOR: controller_user_blog_index]`.

* **Blog Creation:** `/<slug>/create_blog` `[@ANCHOR: UX_CREATE_BLOG_POST]`.

* **Post Creation & Editing:** `POST /<slug>/create_blog_post` creates a blank "New Post" in the owner's blog (or redirects to `/<slug>/blog` if no blog exists yet) and redirects to `/blog_post/edit/<post_id>`, this module's own ownership-scoped title/body/publish form (the website-builder Edit toolbar needs the sitewide `website.group_website_designer` permission, which site owners do not hold).

* **Documentation:** Proxies knowledge records `/user-websites/documentation` `[@ANCHOR: controller_user_websites_documentation]`.

* **Appeals:** User submission endpoint `/website/submit_appeal` `[@ANCHOR: UX_SUBMIT_APPEAL]`.

* **Subscriptions:** `/<slug>/subscribe` `[@ANCHOR: UX_SUBSCRIBE]`. Unsubscribe verification with HMAC-SHA256 token validation over the model, record, partner and timestamp `[@ANCHOR: controller_unsubscribe_digest]`. The timestamp is signed but its age is not checked, so a valid link does not expire.

* **`GET /api/v1/user_websites/pending_reports`**: Returns a JSON object `{'count': int, 'latest_id': int, 'url': str}` of unhandled violation reports (`url` is the back-office pending queue, `/odoo/action-user_websites.action_content_violation_report_pending`). Restricted to administrators `[@ANCHOR: api_pending_reports]`; any other caller gets `{'count': 0, 'error': 'Forbidden'}` with HTTP 200, not an error status. Verified by `[@ANCHOR: test_admin_violation_toast_rpc]`.

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

* **Owner toolbar** `[@ANCHOR: user_websites:UX_OWNER_TOOLBAR]` (`views/owner_site_templates.xml`, called from `user_navbar`): shown above the site's own navbar to the site's owner, or a member of the owning group, on every page of that site (an internal user sees it on their own site only). `?visitor=1` hides it.
* **Dashboard** `/my/site/<slug>` `[@ANCHOR: user_websites:COMM_owner_site_dashboard]`, `/my/site` redirects to the caller's own. Page editor `/my/site/<slug>/page/new|<id>` `[@ANCHOR: user_websites:UX_OWNER_PAGE_EDITOR]`, save `[@ANCHOR: user_websites:COMM_owner_page_save]`, publish toggles `[@ANCHOR: user_websites:COMM_owner_page_publish]` and `[@ANCHOR: user_websites:COMM_owner_post_publish]`, image upload `[@ANCHOR: user_websites:COMM_owner_image_upload]` (`controllers/owner_site.py`). Every route resolves the site from the database and answers 404 unless the caller owns it, or belongs to the owning group (`[@ANCHOR: user_websites:owner_resolve_site]`); an administrator who is not the owner gets the 404 too. The owner of a suspended or not-yet-eligible site gets a 403 page that says why instead (hidden from everyone else); a retired slug redirects to the live one.
* **Editor** `[@ANCHOR: user_websites:UX_RICH_EDITOR]`: a contenteditable WYSIWYG (`static/src/js/rich_editor.js`), no raw HTML and no external library. Odoo's own `html_editor` is an OWL backend component tied to the website builder's `website.group_website_designer` permission and its asset bundles; mounting it for portal members would hand them the sitewide editor, which this module deliberately never grants.
* **Sanitizer** `[@ANCHOR: user_websites:member_html_sanitize]` (`models/member_html.py`): strict allow-list in the request path for every caller, administrators included; hostile input is also filed through `website.page._trigger_malicious_arch_violation()`.
* **Limits**: title 120 characters, body 200,000 characters, images JPEG/PNG/GIF/WebP at most 2 MB and 100 per site, 30 writes and 10 uploads per user per minute (Redis counters, `[@ANCHOR: user_websites:owner_rate_limit]`; Redis down answers 503).
* **Retention**: no route erases. Unpublish keeps the record as a draft. Delete (`POST .../<page|post>/<id>/delete`, with a confirmation step) takes a page or post off the site and every public surface but archives it (`active` false, `deleted_by_owner` stamped; a page is moved to a private address so its own can be reused), and the dashboard's Deleted list can restore it, unpublished. The home page cannot be deleted. Uploaded images are public `ir.attachment` rows owned by the account (`res_model='res.users'`/`user.websites.group`, description `user_websites site image`); a member's account-erasure flow removes the images owned by that member's own account (not a group's images).
* **Layout** `[@ANCHOR: user_websites:member_site_standalone_layout]`: on a member site `layout_inherit_user_navbar` sets `no_header`/`no_footer`, so hams.com's header and footer are not rendered, and the site's own header (`user_navbar`) and footer (`member_site_footer`) are. Assets: `static/src/css/member_site.css` and `rich_editor.js` in `web.assets_frontend`.
* **Home address** `/<slug>` `[@ANCHOR: user_websites:COMM_user_home_fallback]`; `/<slug>/home` answers 301 `[@ANCHOR: user_websites:COMM_user_home_legacy_redirect]`.
* **Owner-site internals**: `/my/site` `[@ANCHOR: user_websites:COMM_owner_site_entry]`; the new-page and edit-page forms `[@ANCHOR: user_websites:COMM_owner_page_new]`, `[@ANCHOR: user_websites:COMM_owner_page_edit]`; a page's arch is the fixed member-page frame around sanitized body markup `[@ANCHOR: user_websites:owner_page_arch]` and the editor reads the body back out of it `[@ANCHOR: user_websites:owner_page_arch_body]`; uploads are accepted by magic number, not by file name or declared type `[@ANCHOR: user_websites:owner_image_mimetype]`.

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
* **Editor functions** (`static/src/js/rich_editor.js`, exercised by `[@ANCHOR: test_tour_owner_site]`): `[@ANCHOR: user_websites:rich_editor_apply_link]`, `[@ANCHOR: user_websites:rich_editor_clean_node]`, `[@ANCHOR: user_websites:rich_editor_close_link_bar]`, `[@ANCHOR: user_websites:rich_editor_image_picked]`, `[@ANCHOR: user_websites:rich_editor_link_bar_click]`, `[@ANCHOR: user_websites:rich_editor_link_keydown]`, `[@ANCHOR: user_websites:rich_editor_open_link_bar]`, `[@ANCHOR: user_websites:rich_editor_paste]`, `[@ANCHOR: user_websites:rich_editor_remember_range]`, `[@ANCHOR: user_websites:rich_editor_restore_range]`, `[@ANCHOR: user_websites:rich_editor_say]`, `[@ANCHOR: user_websites:rich_editor_start]`, `[@ANCHOR: user_websites:rich_editor_sync_field]`, `[@ANCHOR: user_websites:rich_editor_toolbar_click]`, `[@ANCHOR: user_websites:rich_editor_toolbar_state]`, `[@ANCHOR: user_websites:rich_editor_wrap_code]`.
* **Helper functions** (owner interface and sanitizer, each exercised by `tests/test_owner_site.py`): `[@ANCHOR: user_websites:fn_main_serve_core_fallback]`, `[@ANCHOR: user_websites:fn_owner_escape]`, `[@ANCHOR: user_websites:fn_owner_escape_attr]`, `[@ANCHOR: user_websites:fn_owner_svc]`, `[@ANCHOR: user_websites:fn_owner_owner_domain]`, `[@ANCHOR: user_websites:fn_owner_qcontext]`, `[@ANCHOR: user_websites:fn_owner_report_if_hostile]`, `[@ANCHOR: user_websites:fn_owner_clean_body]`, `[@ANCHOR: user_websites:fn_owner_own_page]`, `[@ANCHOR: user_websites:fn_owner_back_to_editor]`, `[@ANCHOR: user_websites:fn_owner_attachment_owner]`, `[@ANCHOR: user_websites:fn_hmtl_clean_url]`, `[@ANCHOR: user_websites:fn_hmtl_is_hostile]`, `[@ANCHOR: user_websites:fn_hmtl_sanitize_member_html]`, `[@ANCHOR: user_websites:fn_hmtl_clean_children]`, `[@ANCHOR: user_websites:fn_hmtl_clean_attributes]`, `[@ANCHOR: user_websites:fn_hmtl_drop]`, `[@ANCHOR: user_websites:fn_hmtl_unwrap]`, `[@ANCHOR: user_websites:fn_hmtl_menu_pages]`, `[@ANCHOR: user_websites:fn_users_user_websites_menu_pages]`, `[@ANCHOR: user_websites:fn_group_user_websites_menu_pages]`.
* **SVG schematics** survive the sanitizer through zero_sudo's SVG allow-list `[@ANCHOR: user_websites:member_html_svg_allowlist]`; every other active or unknown element is removed.

* `[@ANCHOR: user_websites:COMM_owner_ascii_digits]` `_is_ascii_digits(value)` (owner site controller) is True only for a non-empty string of ASCII digits, the strings `int()` accepts; a page id such as the superscript two answers 404, not 500. **Tests:** `test_10b_a_page_id_that_is_not_ascii_digits_is_a_404_not_a_500`.

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

### `slugify(s, max_length=None)` in `edge_routing.utils` [@ANCHOR: utils_slugify]

**Purpose:** the one slug generator for user and group site addresses (the first path segment such as `yourname`). It is a local copy so the module does not depend on Odoo's `http_routing` slugify, which moves between versions. **Arguments:** `s` (any value; falsy gives `""`), `max_length` (optional int; the slug is cut to that length and trailing `-` removed, and `0` really means an empty slug). **Returns:** `str` made of `a-z`, `0-9` and single hyphens: the text is stripped and lower-cased, accents are folded to ASCII with NFKD (characters with no ASCII form are dropped), every other run of characters becomes one `-`, and leading and trailing hyphens are removed. No errors, no I/O. It does not check reserved words; `RESERVED_SLUGS` and `reserved_slug_words(env)` do that. The code anchor is `edge_routing:utils_slugify`; test `test_utils_slugify`.

### `res.users._get_gdpr_export_data()` [@ANCHOR: res_users_gdpr_export]

**Purpose:** this module's addition to the privacy export of one user (GDPR access request). **Signature:** no arguments besides `self`, which must be one record (`ensure_one`). **Returns:** the dict from the parent chain (compliance and other modules), with the `user` entry extended by `name`, `email` and `website_slug` (False when unset). Pages, blog posts, reports and appeals are not put in this dict: they are streamed by `_get_gdpr_streamed_keys()`, which adds the generators `pages`, `blog_posts`, `submitted_reports` and `appeals` (read in batches of 1000 by id) so a large account cannot exhaust memory. **Errors:** none of its own. Read-only, no locking. Dependent modules storing personal data override the hook and add their own keys. **Tests:** `test_gdpr_export_hook`.

### `test_gdpr_export_hook` [@ANCHOR: test_gdpr_export_hook]

Test `test_01_gdpr_export_hook` in `tests/test_sdk_extensibility.py`. It creates a `website.page` owned by the test user and commits, then calls `_get_gdpr_export_data()` and `_get_gdpr_streamed_keys()` on that user. It asserts that the data has a `user` key and that `pages` appears among the streamed keys, so large content lists are streamed rather than loaded into the dict (the message names the out-of-memory risk). It is cited from the hook in `models/res_users.py` and tests `res_users_gdpr_export`. A failure means the hook or the stream key was removed.

### `res.users._execute_gdpr_erasure()` [@ANCHOR: gdpr_sudo_erasure]

**Purpose:** hard-deletes everything this module holds for one user when they exercise the right to erasure. **Signature:** `self` is one `res.users` record; no other arguments; returns `None`. **Steps:** as the `zero_sudo.gdpr_service_internal` service account (so the search finds the target's rows whoever the caller is, not only the caller's own), it loops over `website.page`, `blog.post` and `blog.blog` owned by the user, 5000 at a time, unlinking each batch inside a savepoint; then removes up to 500 site-editor images (`ir.attachment` with description `user_websites site image` on the user); then sets `privacy_show_in_directory` False. **Errors and locking:** a concurrent-update, serialization or deadlock error is retried after 0.5 s; any other error is raised. **Side effects:** commits after each batch outside test mode and pauses 0.1 s between full batches unless `ODOO_DISABLE_SLEEPS` is set. Group-owned content is not deleted. **Tests:** `test_gdpr_erasure_pages`, `test_gdpr_erasure_posts`.

### `test_gdpr_erasure_pages` [@ANCHOR: test_gdpr_erasure_pages]

Test `test_02_right_to_erasure` in `tests/test_privacy_gdpr.py` (a page half of it; the source comment in `res_users.py` cites this anchor). An authenticated member with a page and a post POSTs to `/my/privacy/delete_content` with a CSRF token. The test asserts HTTP 200 landing on `/privacy/erased` with the text "Your data has been erased", the account deactivated, the page record gone (`exists()` False) and the directory flag off. It also cites `gdpr_sudo_erasure` and `UX_GDPR_ERASURE`. Another test, `test_01_gdpr_erasure_suspended_user` in `tests/test_audit_edge_cases.py`, checks that a suspended user's content is unlinked, not merely unpublished.

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
