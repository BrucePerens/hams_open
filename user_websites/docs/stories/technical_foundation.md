# Story: Technical Foundation and Utilities

As a **Developer**, I want to use reliable utility functions and a robust security model so that the module remains maintainable and secure.

Each anchor below has its own entry, read from the code the anchor points at. Source paths are relative to the
`hams_open` repository root. Test code cites each anchor with a "Tests" comment.

## Scenarios

### Slug Generation: `slugify` [@ANCHOR: edge_routing:utils_slugify]

- **When** a user's name or a group's name needs to be converted into a URL-friendly slug.
- **Then** the system uses a robust `slugify` utility. Verified by `[@ANCHOR: edge_routing:test_utils_slugify]`.
- **And** it handles special characters and normalization consistently.

`slugify(s, max_length=None)` in `edge_routing/utils.py` turns any value into a lower-case ASCII string of letters,
digits and single hyphens. It is a local copy so that nothing depends on `http_routing`'s own slugify, which moves
between Odoo versions.

| Argument | Type | Meaning |
|---|---|---|
| `s` | any | The text; converted with `str()`. A falsy value returns `""`. |
| `max_length` | int or None | When not None, the slug is cut to that many characters and trailing hyphens are removed. `0` really does truncate to an empty string. |

**Returns** a `str` that is empty or matches `^[a-z0-9]+(-[a-z0-9]+)*$`. Steps: strip, lower-case, NFKD-normalise and drop
non-ASCII (so an accented letter loses its accent, other scripts vanish), collapse every run of other characters into one
hyphen, strip hyphens from both ends. **Errors:** none. It is pure, has no I/O or locking, and is safe from any thread.
It does not check reserved words or uniqueness; `edge.routing.mixin` does that when the slug is stored.

### Documentation Access: `COMM_documentation` [@ANCHOR: user_websites:COMM_documentation]

- **When** I need help using the module.
- **Then** I can navigate to the documentation route. Verified by `[@ANCHOR: test_documentation_route]`.
- **And** the system attempts to redirect me to the appropriate `knowledge.article` if available.

`GET /user-websites/documentation` (`UserWebsitesController.documentation` in `user_websites/controllers/main.py`) needs a
logged-in user (`auth="user"`, `website=True`) and takes no arguments of its own (`**kwargs` is ignored). It searches
`knowledge.article` for the first article whose name matches `User Websites Documentation%` (case-insensitive). The search
runs as the visitor, deliberately not as the service account, so the visitor's own access rights decide what they may see.
If an article with a `website_url` is found the response is a redirect to it; otherwise the built-in QWeb page
`user_websites.documentation_page` is rendered. The `knowledge.article` model must exist: a missing model fails loudly
rather than being hidden. It has no side effects and writes nothing.

## Technical Notes

### Service-Account Ownership Check on Create: `mixin_proxy_ownership_create` [@ANCHOR: mixin_proxy_ownership_create]

The module relies on a specialized service account for most background and initialization tasks. Verified by
`[@ANCHOR: test_mixin_ownership_validation]`.

`_check_proxy_ownership_create(self, vals_list)` in `user_websites/models/user_websites_owned_mixin.py` runs on every
create of a record that carries the owner mixin (`owner_user_id` and `user_websites_group_id`). `vals_list` is the list of
value dictionaries and is edited in place: a missing owner is filled with the current user (ADR 0082) unless the caller is
the public user. It raises `ValidationError` when one record names both an owner and a group, and `AccessError` when a
non-administrator gives no owner, names another user, or names a group they are not a member of (members are read once,
through the service account, to avoid a query per row). Callers that are `su`, `base.group_system`, the module's
administrator group or `group_user_websites_ownership_bypass` skip the membership rules. If the service account is not
provisioned yet (early test setup) the group check is skipped with a debug log. It does not write to the database.

### Pending Reports Endpoint: `COMM_pending_reports` [@ANCHOR: user_websites:COMM_pending_reports]

`GET /api/v1/user_websites/pending_reports` (`pending_reports` in `controllers/main.py`; `auth="public"`, `website=True`,
`sitemap=False` so that the moderation endpoint is never listed in the public sitemap) feeds the moderator toast. It takes
no arguments. Anyone who is not a user-websites administrator or a system administrator, including every visitor and
every logged-out client, gets HTTP 200 with `{"count": 0, "error": "Forbidden"}` (200 on purpose, so a front-end fetch in a
headless tour does not raise). For a moderator it counts `content.violation.report` rows in state `new` through the
service account (capped at 1000) and returns JSON `{"count": n, "latest_id": id_of_newest_or_0, "url":
"/odoo/action-user_websites.action_content_violation_report_pending"}`. It is read-only and uses no lock. The `url` is
a fixed back-office path, never built from input. Verified by `[@ANCHOR: test_admin_violation_toast_rpc]`, which exercises
the same endpoint through the toast code.

### Extensibility Dropzones

The module provides several dropzones for UI extension. A dropzone is an empty element with a stable `id`
(`user_websites_dropzone_<name>`) and an anchor comment, placed in a view the module already extends. Another module adds
its own content with an `xpath` on that `id`, so it does not depend on the surrounding markup. A dropzone renders nothing
by itself, has no arguments and no code behind it. The rendering tests in `user_websites/tests/test_xpath_rendering.py`
compile each view and assert the element is present.

### Dropzone Home Header: `dropzone_home_header` [@ANCHOR: dropzone_home_header]

The element `user_websites_dropzone_home_header` is the first child inside `website.layout` in the
`user_websites.template_default_home` template (`views/user_websites_templates.xml`), the master layout every member or
group home page is rendered through. Content placed here appears above the page body (`t-out="0"`), for example a banner.
It is tested in `test_xpath_rendering.py` (`test_dropzone_home_header`).

### Dropzone Home Footer: `dropzone_home_footer` [@ANCHOR: dropzone_home_footer]

The element `user_websites_dropzone_home_footer` is the last child inside `template_default_home`, right after the page
body. A module that wants a signature, a counter or a related-links block under every member home page adds it here. It
renders only on home pages that use the default home layout. It is tested in `test_xpath_rendering.py`
(`test_dropzone_home_footer`).

### Dropzone Global Navbar: `dropzone_navbar` [@ANCHOR: dropzone_navbar]

The element `user_websites_dropzone_navbar` sits directly after `<header>` in the `layout_inherit_user_navbar` view, which
extends `website.layout` and so is on every website page. The member-site navbar (`user_websites.user_navbar`) is called
right after it, and only when the request resolves to a member site. Use it for content that must appear site-wide above
the navbar. Tested by `test_dropzone_navbar` in `test_xpath_rendering.py`.

### Dropzone Navbar Actions: `dropzone_navbar_actions` [@ANCHOR: dropzone_navbar_actions]

The element `user_websites_dropzone_navbar_actions` is inside the member-site navbar template `user_websites.user_navbar`,
after the list of menu entries (Home, Blog and the member's published pages). It is for extra buttons or links on the
right-hand end of a member site's navbar, such as a follow button. It is only rendered on member sites, never on the main
site. Tested by `test_dropzone_navbar_actions`.

### Navbar Owner Lookup: `mixin_navbar_profile_user` [@ANCHOR: mixin_navbar_profile_user]

A member-owned record's navbar resolves its owner through the service account, so logged-out visitors and other members
can load the page. Verified by `[@ANCHOR: test_navbar_member_owned_record_public_visitor]`.

`_user_websites_navbar_profile_user(self)` in `user_websites/models/user_websites_owned_mixin.py` takes no arguments and
needs a single record (`ensure_one`). It reads only the owner's id from the record, in the caller's own environment (which
the visitor could already load), then returns that `res.users` row browsed as the `user_websites_service_account`, or
`False` when the record has no user owner. It exists because the public user and portal members cannot read another
member's user row, so reading `owner_user_id` fields directly gave a 403 to everyone except the owner. The navbar template
reads the owner's slug, name, avatar and write date from the returned record. It performs no write and takes no locks; the
service environment lookup is cached by `zero_sudo`.

### Dropzone Portal Templates: `dropzone_templates` [@ANCHOR: dropzone_templates]

The element `user_websites_dropzone_templates` is in the `portal_my_home_appeal` view (`views/user_websites_templates.xml`),
which extends `portal.portal_my_home` and is placed before the "my home" block of the portal. It lets another module add a
block, such as a notice, at the top of the logged-in user's portal home page where the user's own appeal status is also
shown. Tested by `test_dropzone_templates` in `test_xpath_rendering.py`.

### Dropzone Snippets Sidebar: `dropzone_snippets` [@ANCHOR: dropzone_snippets]

The element `user_websites_dropzone_snippets` is the first child inside the `user_websites_snippet_category` view
(`views/snippets.xml`), which extends `website.snippets`, the building-block panel of the website editor. It is placed
before the module's own "Profile Widgets" panel, so another module can add its own panel of snippets in the same sidebar.
Tested by `test_dropzone_snippets`.

### Dropzone Website Layout: `dropzone_layout` [@ANCHOR: dropzone_layout]

The element `user_websites_dropzone_layout` is added after `<footer>` by the `layout_inherit_report_violation` view of
`website.layout` (`views/website_layout.xml`), immediately before the report-violation modal is called in. It is the place
for site-wide markup that belongs at the end of every page, such as a script holder or a second modal. It exists on every
website page. Tested by `test_dropzone_layout`.

### Dropzone User Settings: `dropzone_users` [@ANCHOR: dropzone_users]

The element `user_websites_dropzone_users` is the first child of the "User Websites" notebook page that the
`view_users_form_user_websites` view adds to the back-office user form (`base.view_users_form`, in
`views/res_users_views.xml`). It sits above the Configuration group (slug, page limit, directory opt-in). Another module
can add fields or buttons for per-user website settings here. Visible only to people who can open the user form. Tested by
`test_dropzone_users` (`test_02_res_users`).

### Dropzone Blog Post Form: `dropzone_blog_post` [@ANCHOR: dropzone_blog_post]

The element `user_websites_dropzone_blog_post` sits right after the `owner_user_id` and `user_websites_group_id` fields
that the `view_blog_post_form_inherit_user_websites` view adds to the back-office blog post form (`website_blog`'s
`view_blog_post_form`, in `views/blog_post_views.xml`). Add extra fields about a post's ownership or publishing here.
Tested by `test_dropzone_blog_post` (`test_03_blog_post`).

### Dropzone Directory Card: `dropzone_directory_card` [@ANCHOR: dropzone_directory_card]

The element `user_websites_dropzone_directory_card` is inside each member card of the public `/community` directory
(`community_directory` template in `views/user_websites_templates.xml`), under the "View Blog" link. It is rendered once
per directory entry, so content placed here repeats for every listed member and has the entry in scope. Only members who
opted in appear in the directory. There is no dedicated test of its markup yet; the directory page tour renders it
(to-do: add a `test_xpath_rendering.py` assertion).

### Dropzone Global Settings: `dropzone_settings` [@ANCHOR: dropzone_settings]

The element `user_websites_dropzone_settings` is the first child inside the form of the "User Websites" block that
`res_config_settings_view_form_inherit_user_websites` adds to the back-office settings screen
(`base.res_config_settings_view_form`, in `views/res_config_settings_views.xml`), above the page-limit, blog-limit and
post-limit settings. Another module adds its own global setting here. Only settings-capable users see it. Tested by
`test_dropzone_settings`.

### Performance and Reliability

The entries in this section describe the caches, crons, and guards that keep member sites fast and safe.

### Slug Invalidation on Unlink: `slug_cache_invalidation_unlink` [@ANCHOR: slug_cache_invalidation_unlink]

Distributed caches are invalidated upon slug mutation. `res.users` mixes in `edge.routing.mixin`, whose
`get_record_by_slug(slug)` is wrapped in `@distributed_cache()` (a per-process RAM cache plus Redis): a repeat lookup of a
known slug runs no SQL. The mixin's `unlink()` (`edge_routing/models/routing_mixin.py`, `edge_routing:COMM_mixin_unlink`)
collects the non-empty `website_slug` values before the delete and then calls `zero_sudo.security.utils`
`_notify_cache_invalidation(model, slugs)`, which clears the local entries after commit and sends a Postgres `NOTIFY` that
the `distributed_redis_cache` daemon forwards to every other worker. A failure to notify is logged as a warning and does
not undo the delete. The test `test_04_bdd_ormcache_query_counting_slugs` in `tests/test_audit_edge_cases.py` proves zero
queries on a cache hit and a fresh query after the slug changes.

### Slug Invalidation for Groups: `group_slug_cache_invalidation_unlink` [@ANCHOR: group_slug_cache_invalidation_unlink]

The same mechanism covers group sites: `user.websites.group` also uses `edge.routing.mixin`, so deleting a group calls the
mixin `unlink()`, which sends the cache-invalidation notice for the group's slug on the model `user.websites.group`. Without
it another worker would keep resolving a deleted group's slug to a record that no longer exists until its cache expired. The
notice is best effort: if `pg_notify` fails, a warning is logged and the delete still stands. Slugs are collected before the
delete because they are gone afterwards. Test: `test_05_bdd_ormcache_query_counting_group_slugs` in
`tests/test_audit_edge_cases.py` (zero SQL on a hit, SQL again after a change).

### View Counter Sync: `ir_cron_flush_view_counters` [@ANCHOR: ir_cron_flush_view_counters]

High-throughput view counters are flushed to the database via cron. The record `ir_cron_flush_view_counters`
(`data/user_websites_data.xml`) runs `model._flush_redis_view_counters()` on `website.page` every 15 minutes as the
`user_websites_service_account`. The method scans Redis for `views:<db>:page:*` keys (at most 1000 per run), reads them in
one pipeline, deletes (via a decrement script) each drained key first and then calls the Postgres procedure, so a failure
loses views rather than counting twice. It commits itself outside test mode and rolls back on Redis or Postgres errors. When
a full batch was drained it re-triggers itself once, so a backlog clears without waiting 15 minutes; an empty scan never
re-triggers. The test is `test_cron_redis_flush` in `tests/test_audit_edge_cases.py`.

### View Counter Procedure: `procedure_flush_view_counters` [@ANCHOR: procedure_flush_view_counters]

Database round-trips are optimized via a PostgreSQL procedure. The server-action record `procedure_flush_view_counters`
(`data/procedures.xml`, `noupdate`) runs on every module update and executes `CREATE OR REPLACE FUNCTION
user_websites_flush_view_counters(updates JSONB) RETURNS VOID`. The argument is a JSON array of `{"inc": int, "pid": int}`
objects; for each it adds `inc` to `website_page.view_count` (null counts as 0) where `id = pid`. The flush cron calls it
as `SELECT user_websites_flush_view_counters(%s)` with the JSON text, so a whole batch is one round trip. An unknown `pid`
updates no row and is not an error. There is no explicit lock beyond the row locks of the updates. Tested in
`tests/test_appeals_and_views.py` (view counter flush).

### Weekly Digest: `ir_cron_send_weekly_digest` [@ANCHOR: ir_cron_send_weekly_digest]

Automated subscriptions are handled by a robust cron process. The record `ir_cron_send_weekly_digest`
(`data/user_websites_data.xml`) runs `model.send_weekly_digest()` on `blog.post` once a week as the service account. The
method (`models/blog_post.py`) reads up to 50 digest rows after the saved cursor `user_websites.last_digest_id` from the
`user_websites.weekly_digest_view` SQL view, signs a one-click unsubscribe URL with HMAC-SHA256 using the instance crypto
secret, queues one mail per subscriber with `List-Unsubscribe` headers (not sent inline), then advances the cursor and
re-triggers the cron if the batch was full, otherwise resets the cursor to 0. Without a crypto secret it logs an error and
sends nothing. Tested by `test_cron_batching_resumption` in `tests/test_audit_edge_cases.py`.

### Moderation Alerts: `ir_cron_notify_pending_reports` [@ANCHOR: ir_cron_notify_pending_reports]

Admins are notified of pending reports. The record `ir_cron_notify_pending_reports` (`data/user_websites_data.xml`) runs
`model._cron_notify_pending_reports()` on `content.violation.report` once a day as the service account. For each company the
service account belongs to it counts reports in state `new`; if any exist it queues one summary mail
(`email_template_pending_violations_summary`) to the system parameter `user_websites.company_abuse_email`, falling back to
the company email and then `admin@example.com`. It sends nothing when the queue is empty and does not change any report.
The mail goes out through the mail service account and is queued, not forced. Tested by `test_08_cron_pending_reports` in
`tests/test_audit_edge_cases.py`.

### Simulation: `simulation_environment` [@ANCHOR: simulation_environment]

End-to-end operational load is verified in the simulation environment. The test class `TestLongRunningSimulation`
(`tests/test_simulation.py`, tagged `simulation`, excluded from the standard run) drives sign-up, page and blog creation,
reports, strikes, suspensions and appeals in a loop and times each step. `SIMULATION_DURATION_MINUTES` (default 60) is
really an iteration count; `SIMULATION_MAX_AVG_TIME` (default 0.5 seconds) is the per-step average above which a speed
regression warning is logged. It runs in one transaction with a flush at each step, takes the whole test database for its
duration, and creates only test data. Run it on demand, never in the default gate.

### Sanitization: `website_page_sanitize_arch` [@ANCHOR: website_page_sanitize_arch]

User-provided HTML is sanitized to prevent XSS. `_sanitize_user_arch(self, arch_content)` in `models/website_page.py`
(`@api.model`) returns a pair `(clean_arch, was_modified)`. Empty input comes back unchanged with `False`. Otherwise the
text is parsed leniently, inline SVG is rebuilt from the `zero_sudo` allow-list, elements with an undeclared namespace
prefix and `script`, `iframe`, `object`, `embed` and `base` are removed, `<link>`, risky `style` attributes and unlisted QWeb
`t-*` directives are filtered, and every `<` in text is re-spelled `&lt;` so a text node cannot become markup. It is
idempotent. A parse failure gives `"<div>Sanitization Error</div>"` and `True`. `create` and `write` call it; a modified
result files an automatic violation report through `_trigger_malicious_arch_violation`. Tests: `test_orm_security.py`.

### Reentrancy Guard on Publish Toggle: `website_page_write_reentrancy_guard` [@ANCHOR: website_page_write_reentrancy_guard]

Writing `website_published` on a page recurses once through the ORM's own `write()` (Odoo's compute/inverse plumbing turns
it into a second, inner write of `is_published`), so the publish/unpublish invalidation signal is guarded to fire exactly
once per logical toggle rather than twice. In `website.page.write` (`models/website_page.py`) the guard is the context key
`_website_page_cache_invalidation_in_progress`: absent on the outer call, set to `True` on the inner `super().write()`, so
only the outer call sends `_notify_cache_invalidation("website.page", urls)` and the Cloudflare purge. The key is only passed
down, never stored, and needs no lock. If the service account cannot be resolved the write falls back to the caller's own
environment with the same context key. The tests count the signals for one publish toggle.

### Toast Notifications: `toast_notifications_logic` [@ANCHOR: toast_notifications_logic]

Feedback is provided via native notifications. `_checkUrlForNotifications()` in `static/src/js/toast_notifications.js`
(class `UrlToastNotification`, a public interaction on `#wrapwrap`) runs at page start, reads the query string, and maps
one parameter to a message: `report_error=<reason>` (a danger toast using the `REPORT_ERRORS` table, unknown reasons show a
default), `report_submitted`, `appeal_submitted`, `subscribed` and `erased`. It shows the message through Odoo's
notification service (non-sticky) and then rewrites the address with `history.replaceState` so a refresh does not repeat
it. No parameter means no toast and no change. Parameters only pick a canned message, so they cannot inject text. Tested by
the `toast_notifications_tour.js` tour (`test_tour_toast_notifications`).

### Admin Violation Toast: `admin_toast_logic` [@ANCHOR: admin_toast_logic]

Feedback is provided to moderators the same way. `_checkPendingReports()` in `static/src/js/toast_notifications.js`
(class `AdminViolationToast`) fetches `/api/v1/user_websites/pending_reports`, and shows a sticky warning with "Review
reports" and "Dismiss" buttons when `shouldShowToast(seen, data, now)` says so: the queue is not empty and it was never
shown, or 12 hours have passed, or the count or newest report id grew. The last view is kept in `localStorage` under
`admin_violation_toast_seen` (reads and writes are wrapped, so blocked storage only means the toast repeats). The button
link is used only if the server's `url` starts with `/`. Network or server errors are ignored silently. Verified by
`[@ANCHOR: test_admin_violation_toast_rpc]` in `tests/test_controllers.py`.

### Violation Reporting: `violation_report_logic` [@ANCHOR: violation_report_logic]

Form submission is handled securely. In the browser, `_onModalShow(ev)` in `static/src/js/violation_report.js` (class
`ViolationReportModal`, selector `.user-websites-report-container`) runs when the Bootstrap report modal opens: it copies the
`data-url` of the button that opened it into the hidden `url` input, and clears the description and email fields, so a
previous report's text is never shown or resent. It does nothing if no button opened it. The server side is
`POST /website/report_violation` in `controllers/main.py` (CSRF protected): a filled honeypot field is silently treated as
success, then `form_shield` checks, then structural checks, then the report is created through the service account with
the description cut to 5000 characters. Tested by `test_tour_violation_report`.

## Domain List API for Certificate Maintenance

### `COMM_api_domains` [@ANCHOR: user_websites:COMM_api_domains]

`GET /api/v1/user_websites/domains` (public, no CSRF) returns `{"domains": [...]}` for the certificate
automation: every `edge.routing.domain` name plus, when `ham_dns` is installed, every `ham.dns.zone` name,
de-duplicated. Reads run as the module's service account and page through the tables by id, 5000 rows at a
time, so no domain past the 5000th is silently left off (which would quietly stop its certificate renewing).
A failure reading the DNS zones is logged and the routing domains are still returned. The handler is `api_domains` in
`controllers/user_websites_api.py`; it takes no arguments and only answers GET. The result is a JSON object with HTTP 200;
the order of names is not defined because duplicates are removed through a set. The endpoint reveals only names that are
already public through DNS and certificates. Tested by `tests/test_domains_api.py`.
