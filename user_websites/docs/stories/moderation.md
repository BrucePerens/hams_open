# Story: Content Moderation

As a **Site Visitor** or **Administrator**, I want a way to report and manage content violations to ensure the community remains safe and respectful.

Each behaviour below is explained in full in the "Anchor reference" section at the end of this file, one entry per anchor (ADR 0113). The entry names are given in square brackets.

## Scenarios

### Reporting a Violation
- **Given** I am viewing a community member's page
- **When** I encounter inappropriate content
- **Then** I can click the "Report Violation" button and provide details about the issue [report_violation controller].
- **And** the system records my report and notifies administrators.
- **But** the button only ever appears when the page I'm viewing is genuinely personal-website content, not any other model that happens to reuse the same ownership mixin purely for claim-tracking (an event, a repeater, a classifieds listing, a training-progress record, and more all adopt it without being personal-site pages at all) [report button scoping].

### Reviewing Abuse Reports
- **Given** I am a **User Websites Administrator**
- **When** I log in, I see a sticky "Pending Moderation" toast with a "Review reports" button linking to the pending queue and a "Dismiss" button, shown at most once per 12 hours unless the queue grows [pending-moderation toast; pending-reports endpoint].
- **When** I review a report, I can take action against the content owner.
- **Then** I can issue a strike, which may lead to automatic account suspension. The strike is applied by the generic `content_moderation` module, moved there 2026-09-23; this module's own `_apply_enforcement_action()` override is what actually strikes and, past 3 strikes, suspends [enforcement hook and strike].

### Automated Security Enforcement
- **Given** a user attempts to save a page with malicious code (SSTI/XSS)
- **When** the system sanitizes the architecture [page architecture sanitizer]
- **Then** it automatically triggers a security violation report and strikes the user's account for attempting to bypass security [enforcement hook and strike].

### Appealing a Suspension
- **Given** my account has been suspended from the websites platform
- **When** I visit my portal
- **Then** I can submit a moderation appeal to explain my case to the administrators [appeal controller].

## Technical Notes
- The "take action" consequence is an extension hook owned by the generic `content_moderation` module: its own default is a documented no-op (no strike infrastructure exists to assume outside a consuming module), and this module's own override supplies the real strike-and-suspend behavior described above.
- Pending reports are checked via a background RPC call.
- Administrators receive periodic email notifications about outstanding reports [admin digest cron].
- A report submission redirects the visitor back to the page they reported (never to a bare `/`, and never to an attacker-controlled off-site host: only the path portion of the submitted URL is ever trusted) [redirect target].
- The site's own mail templates never use a made-up sender or recipient address [mail sender fallback].

## Anchor reference

### `report_violation(self, url="", reason="", description="", email="", website_honeypot="", **post)` [@ANCHOR: user_websites:UX_REPORT_VIOLATION]

The public form endpoint behind the "Report Violation" button, `POST /website/report_violation` in `controllers/main.py` (`auth="public"`, `website=True`, `csrf=True`, so the CSRF token is checked by Odoo). Verified by `[@ANCHOR: test_tour_violation_report]`, the browser tour of the report modal (`tests/test_ui_tours.py`), and its failure branch is cited there too. Visitors, signed in or not, use it to tell administrators about a page.

| Argument | Type | Meaning |
|---|---|---|
| `url` | str | The reported page. Empty means the `Referer` header is used. Reduced to its path and checked to be a page of this site. |
| `reason` | str | Accepted from the modal; this handler does not store it (unknown whether a later version will, to-do: decide). |
| `description` | str | What is wrong. Required; cut to 5000 characters. |
| `email` | str | Optional contact address; required and format-checked for guests. Stored as `reported_by_email`. |
| `website_honeypot` | str | Hidden field; any value means a program filled the form, and the report is discarded with a success redirect. |
| `**post` | dict | The `form_shield` fields (proof of work or question answer, signature, time stamps). |

Returns an HTTP redirect, always back to the reported page's path with a query flag: `report_submitted=1` on success (and for a silent honeypot refusal), `report_error=<reason>` otherwise (`missing_fields`, a `report_problem` reason, `creation_failed`). A refusal by the shield is answered by `_report_refusal`. Errors: a `KeyError` or `ValueError` while creating the row is logged and becomes `report_error=creation_failed`. Side effects: creates one `content.violation.report` row with state `new` through the module's service account, resolving the owner or group from the first path segment, and records the signed-in reporter from the session, never from the form. Security: the redirect cannot leave the site (see the redirect-target entry).

### Report button scoping [@ANCHOR: report_violation_scoped_to_personal_website_content]

A rule in the QWeb layout inheritance `views/website_layout.xml` (an xpath before the footer) that decides whether the report modal and its help link are rendered. The earlier check, "the main object carries the ownership mixin", was too wide: events, repeaters, classifieds listings and training records adopt the mixin purely for claim tracking and the widget leaked onto them with a link to personal-website documentation. The rule now shows the widget only when the main object is a content model this module defines (`website.page`, `blog.blog`, `blog.post`), or is itself the profile owner record (`profile_user` or `profile_group`), which is what the profile home and blog listing routes set. It compares the object with the profile record rather than keeping a model-name list, so it cannot drift. Test: `tests/test_xpath_rendering.py` cites this anchor. It runs at render time, has no side effects and no arguments.

### The pending-moderation toast: `_checkPendingReports()` [@ANCHOR: admin_toast_logic]

The method `AdminViolationToast._checkPendingReports()` in `static/src/js/toast_notifications.js`, started once on every page (`#wrapwrap`) by `start()`. It fetches `/api/v1/user_websites/pending_reports`, throws away a non-2xx answer, and asks `shouldShowToast(readSeen(), data, now)` whether to show. If so it adds a sticky Odoo notification titled "Pending Moderation" (type `warning`, text "There are N pending violation reports requiring review.") with a primary "Review reports" button that navigates to `data.url` (only when it is a string beginning with `/`) and a "Dismiss" button, then records the sighting with `writeSeen`. It returns a promise; network errors are swallowed on purpose so a failure never disturbs the page. Takes no arguments. Tests: `[@ANCHOR: test_tour_toast_notifications]` (the tour `toast_notifications_tour.js`, run from `tests/test_ui_tours.py`) and `[@ANCHOR: test_admin_violation_toast_rpc]` (`tests/test_controllers.py`, which proves the endpoint answers an administrator with a JSON `count`).

### `pending_reports(self, **kwargs)` [@ANCHOR: user_websites:COMM_pending_reports]

`GET /api/v1/user_websites/pending_reports` in `controllers/main.py`, the data source for the toast above. Declared `auth="public"`, `website=True` and `sitemap=False` (a 2026-09-23 production bug had listed this admin API in the public sitemap). Anyone who is not a user-websites administrator or system administrator gets HTTP 200 with `{"count": 0, "error": "Forbidden"}`, never an error status, so the page's fetch cannot raise a traceback during UI tours. For an administrator it counts `content.violation.report` rows in state `new` (limit 1000, so the count tops out at 1000) through the service account, and returns JSON `{"count": n, "latest_id": id of the newest new report or 0, "url": the back-office pending queue action}`. Read only, no locks. Test: `tests/test_controllers.py::test_01_api_pending_reports_admin_access` cites this anchor.

### The enforcement hook and the strike [@ANCHOR: content_moderation:action_take_action_and_strike]

Four anchors describe one chain. The heading's anchor is the button method on the generic `content.violation.report` model in the `content_moderation` module (not in this repository tree): it refuses a report already in a terminal state, sets it to `action_taken` and then calls the hook `[@ANCHOR: content_moderation:apply_enforcement_action]`, `_apply_enforcement_action()`, whose own default there only writes an audit note. This module overrides it as `ContentViolationReport._apply_enforcement_action(self)` `[@ANCHOR: user_websites:COMM_apply_enforcement_action]` in `models/content_violation_report_moderation.py`. With `ensure_one()` it increments the strike count of the content owner (`res.users`) or, if only a group is set, of the `user.websites.group`, using the `increment_strike_count()` stored procedure (a row lock, so concurrent moderators count correctly), publishes a cache invalidation, and at 3 or more strikes calls `action_suspend_user_websites()` or `action_suspend_group_websites()` if not already suspended. It then posts a chatter note as the internal mail service account. A report with neither owner nor group falls back to the generic note. Returns nothing. The same chain runs when the page sanitizer detects an injection. Test: `[@ANCHOR: test_moderation_suspension]` (`tests/test_moderation.py::test_01_three_strikes_suspension`), which walks three strikes to suspension.

### `_sanitize_user_arch(self, arch_content)` [@ANCHOR: website_page_sanitize_arch]

Model method on `website.page` (`models/website_page.py`, `@api.model`) that cleans a page body before it is saved and returns `(cleaned_arch, was_modified)`. Empty input returns `(arch_content, False)`. It parses the body leniently, rebuilds every inline SVG from an allow-list, drops elements with undeclared namespace prefixes, removes `script`, `iframe`, `object`, `embed` and `base` (any letter case), removes `link`, rebuilds `style` text and `style` attributes through the stylesheet filter, strips `http-equiv="refresh"` from `meta` and dangerous attribute values, and continues with further checks listed in the source. `was_modified` is true when anything was removed; the caller that sees an attempt to inject code files the automatic security violation report described in the enforcement entry, while plain style or link removals are not counted as attacks. Pure on its input, no database access. Tests: `tests/test_orm_security.py`, and the `test_website_page_sanitize_arch*` anchors cited in the source.

### `submit_appeal(self, reason="", group_id=None, **post)` and its test [@ANCHOR: UX_SUBMIT_APPEAL]

`POST /website/submit_appeal` in `controllers/main.py`, signed-in users only (`auth="user"`, CSRF checked). It lets a suspended member explain themselves; `reason` is the text and `group_id` is the optional site group the appeal is for (the user must be a member of it). An empty `reason` redirects to `/my/home?error=missing_reason`; a group the user is not a member of, or a bad id, redirects to `?error=appeal_failed`; success creates a `content.violation.appeal` in state `new` (user or group set) through the service account and redirects to `/my/home?appeal_submitted=1`, which the URL toast shows as "Submitted". `KeyError` and `ValueError` are logged and give `appeal_failed`. Test: `[@ANCHOR: test_tour_moderation_appeal]`, the tour `static/tests/tours/moderation_appeal_tour.js` that fills the suspension alert's appeal form on `/my/home` and submits it.

### `_cron_notify_pending_reports(self)` [@ANCHOR: cron_notify_pending_reports]

The scheduled digest, in `models/content_violation_report_moderation.py`, run by the cron record `ir_cron_notify_pending_reports` in `data/user_websites_data.xml`. For each company of the module's service account it counts reports in state `new` and, if the count is above zero, queues the pending-violations summary mail to `user_websites.company_abuse_email`, else the company email, else `admin@example.com`, using the internal mail service account (`force_send=False`: it is queued, not sent inline). Iterating the service account's own companies rather than every company stops a company the account cannot see from raising `AccessError` and aborting the run (see ADR 0083). No arguments, returns nothing, creates `mail.mail` rows. Tests: `[@ANCHOR: test_cron_pending_reports]` (`tests/test_audit_edge_cases.py::test_08_cron_pending_reports` and `test_09_cron_pending_reports_ignores_out_of_scope_company`).

### `_report_violation_redirect_target(reported_url, query_param)` [@ANCHOR: user_websites:report_violation_redirect_target]

Module-level helper in `controllers/main.py` that builds where a visitor goes after the report form. `reported_url` is untrusted (a POST field or the `Referer` header), so only its path and existing query string are used, never the scheme or host, through `_reported_path`, which forces exactly one leading slash so `//evil.example` or `/\evil.example` stays on this site; a bare path always resolves against the current site, so there is no open redirect. `query_param` is the flag to append (`report_submitted=1`, `report_error=...`); it is joined with `?`, or `&` when the path already has a query. Returns the string. No side effects, cannot raise on odd input. Test: `[@ANCHOR: test_report_violation_redirects_back_to_reported_page]` (`tests/test_advanced_edge_cases.py`), which posts with no `Referer` and checks the visitor lands on the reported page with `report_submitted=1`.

### Mail sender and recipient fallbacks [@ANCHOR: mail_sender_fallback]

A rule for the three mail templates in `data/user_websites_data.xml` (content violation report, pending-violations summary, weekly digest), written as a comment there and tested by `test_mail_sender_fallback.py`. No template falls back to an invented address. The summary and digest are sent from the company email, then the mail server's own default from address (its `email_from` option); if neither is set the sender is empty, and Odoo refuses to send the mail and records the reason on it. The content violation report is sent from the reporter's own address (signed-in user's email or the address typed in the modal). The abuse report goes to the `user_websites.company_abuse_email` setting, then the company email, then nowhere (the field is empty). This protects a site without a company email from sending mail from, or reports to, a domain it does not own. Configuration only, no code path.
