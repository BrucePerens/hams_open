# Story: Privacy and GDPR Compliance

As a **Community Member**, I want full control over my personal data so that I can exercise my "Right to be Forgotten" and "Right to Portability" as required by GDPR (the EU General Data Protection Regulation).

Each behaviour is explained in full in the "Anchor reference" at the end of this file (ADR 0113). The entry names are given in square brackets.

## Scenarios

### Exporting Personal Data
- **Given** I am logged into my account
- **When** I visit my Privacy Dashboard [privacy dashboard]
- **Then** I can request a full export of my data [export route and button].
- **And** the system generates a machine-readable JSON file containing all my pages, blog posts, and metadata [export data].

### Exercising the Right to Erasure
- **Given** I wish to leave the platform and delete my presence
- **When** I click "Delete My Content" on the Privacy Dashboard [erasure route and button]
- **Then** my account is deactivated first, so a failure part way cannot leave a half-erased account that is still active, and the system then permanently deletes all my hosted content [erasure of content].
- **And** I am shown a public confirmation page stating my data was permanently erased, even though my session is gone [confirmation page].
- **And** I am removed from the public community directory. This module does not anonymize the profile; account handling beyond that belongs to the hams.com onboarding module, if installed.

## Technical Notes
- Data exports use streaming generators to handle large datasets without exhausting server memory.
- Erasure deletes in batches of 5000 records and commits between batches, so even a user with massive amounts of content never holds one huge transaction. As the code stands the batches run inside the delete request itself, not in a separate background job: the "background task" wording of earlier versions of this story is a to-do to settle (either move it to a queued job or drop the claim).

## Anchor reference

### `privacy_dashboard(self, **kwargs)` [@ANCHOR: user_websites:COMM_privacy_dashboard]

`GET /my/privacy` in `controllers/main.py` (`auth="user"`, `website=True`): the member's Privacy and Data page, template `user_websites.portal_my_privacy`. It takes no arguments of its own (`**kwargs` swallows query parameters) and returns the rendered page; a visitor who is not signed in is sent to the login page by Odoo. The page has two cards: "Download My Data" (a form that posts to `/my/privacy/export`) and "Right to Erasure" (warnings, then a confirm-guarded POST form to `/my/privacy/delete_content`), each with a help link into the documentation page. It reads and writes nothing itself. Test: `[@ANCHOR: test_tour_gdpr_privacy]`, the browser tour `gdpr_privacy_tour.js` run from `tests/test_ui_tours.py::test_03_gdpr_privacy_tour`, which opens the page as a member, exports, and erases.

### Export route and "Download My Data" button [@ANCHOR: UX_GDPR_EXPORT]

The user-facing export: the "Export Data to JSON" button on the dashboard (documented in `data/documentation.html` under the id `UX_GDPR_EXPORT`) and the controller `privacy_export(self, **kwargs)` it calls, `/my/privacy/export` [@ANCHOR: user_websites:COMM_privacy_export] (`auth="user"`, `website=True`). It builds the small dictionary part with `user._get_gdpr_export_data()` and the large part from `user._get_gdpr_streamed_keys()` (pages, blog posts, submitted reports, appeals), then returns a streaming `Response` whose body is one JSON object written piece by piece: the dictionary keys first, then each streamed key as a JSON array filled item by item, so memory use stays flat. Headers: `Content-Type: application/json`, `Content-Disposition: attachment; filename="gdpr_export.json"`, `Cache-Control: private, no-store`, so no browser cache, proxy or Cloudflare keeps a person's data. Always exports the signed-in user only, never a user named in the request. Side effects: none, read only. Tests: `tests/test_privacy_gdpr.py` (`test_gdpr_export_api`) and the tour.

### `_get_gdpr_export_data(self)` [@ANCHOR: res_users_gdpr_export]

Extension of the compliance module's export hook on `res.users` (`models/res_users.py`). `ensure_one()`; it takes the base dictionary from `super()`, makes sure there is a `"user"` key, and adds `name`, `email` and `website_slug` (`False` when the member has no slug). Returns the dictionary; pure read, no database writes. The bulk content is not here: it is produced lazily by the sibling `_get_gdpr_streamed_keys()`, which yields in batches of 1000 by ascending id, each batch read in its own cursor as the service account, the member's `pages` (name, url, content), `blog_posts` (name, content, published date), `submitted_reports` (target url, description, status, date) and `appeals` (reason, status, date). Test: `[@ANCHOR: test_gdpr_export_hook]` (`tests/test_sdk_extensibility.py`), `test_01_gdpr_export_hook`, which creates a page for a member and checks that the data has the `user` key with the member's name and that the streamed keys include `pages` and `blog_posts`.

### Erasure route and "Erase My Account & Content" button [@ANCHOR: UX_GDPR_ERASURE]

The user-facing erasure: the red button on the dashboard's Right to Erasure card (documented in `data/documentation.html`, id `UX_GDPR_ERASURE`), guarded by a browser confirmation, and the controller `privacy_delete_content(self, **kwargs)` it posts to, `POST /my/privacy/delete_content` [@ANCHOR: user_websites:COMM_privacy_delete_content] (`auth="user"`, `csrf=True`, so only a form from the site's own page works). It takes the signed-in user from the session (never from the request), calls `user._execute_gdpr_erasure()`, then redirects to `/privacy/erased`; it does not render the dashboard because the account is already deactivated and the login page would hide the result. Errors from erasure propagate as a server error and leave whatever batches were already committed. The dashboard warns that the action is irreversible and that the callsign is banned from enrolling again. Tests: `tests/test_privacy_gdpr.py` (`test_gdpr_erasure_pages` posts the route) and the tour.

### `_execute_gdpr_erasure(self)` [@ANCHOR: gdpr_sudo_erasure]

The erasure itself, `ResUsers._execute_gdpr_erasure` in `models/res_users.py`; no arguments, `ensure_one()`, returns nothing. It runs as the `zero_sudo.gdpr_service_internal` service account for both finding and deleting, so it succeeds for a caller of any privilege (it was changed after a 2026-09-09 review showed a caller outside the target's group would find zero rows). In order it first deactivates the account (`active` false, as the service account, before anything is erased, because the batches commit as they go and a failure part way must not leave a half-erased active account), then removes the member's `website.page`, `blog.post` and `blog.blog` records, each in loops of at most 5000 per savepoint, commits after every batch (except in test mode), pauses 0.1 s between full batches (unless `ODOO_DISABLE_SLEEPS` is set), and on a concurrent-update, serialization or deadlock error waits 0.5 s and retries; any other error is raised. Then it deletes up to 500 site-editor images (attachments on the user described "user_websites site image"), sets `privacy_show_in_directory` to false, and calls `super()` so the compliance module and hams.com add their own steps. Irreversible; not idempotent in effect but safe to repeat. Tests: `[@ANCHOR: test_gdpr_erasure_pages]` and `test_gdpr_erasure_posts` in `tests/test_privacy_gdpr.py`.

### `privacy_erased(self, **kwargs)` [@ANCHOR: user_websites:COMM_privacy_erased]

`GET /privacy/erased` (`auth="public"`, `website=True`) in `controllers/main.py`, the page the erased member lands on. It is public because the account was just deactivated and the session is gone, so any `auth="user"` page would bounce to a login form and the person would never see that the erasure happened. It renders `user_websites.privacy_erased` with an empty context: the page is identical for everyone, ignores the query string, reflects nothing and has no redirect target, so it cannot be used to probe whether an account ever existed. Returns the HTML page; no side effects. Test: `[@ANCHOR: user_websites:test_privacy_erased_public_page]`, defined in `tests/test_privacy_gdpr.py` (cites this route): `test_02b_privacy_erased_public_page` opens the URL without logging in (status 200, the text "Your data has been erased") and checks that a query string carrying a login, email and off-site redirect changes nothing in the page body.

### `_cron_vacuum_tokens(self)` [@ANCHOR: user_websites:gdpr_token_vacuum]

Daily cron `ir_cron_vacuum_gdpr_export_tokens` [@ANCHOR: ir_cron_vacuum_gdpr_export_tokens] (run as the export service account) deletes every `ham.gdpr.export.token` row older than one day; a token is unusable after 5 minutes, used or not, and rows were never removed before. It pages by id, 1000 rows per page, until no old row is left, so there is no cap that leaves rows behind. The method takes no arguments and returns nothing; it is an `api.model` method, so it ignores `self`'s records. It computes a cutoff of now minus one day, then repeatedly searches for tokens with `id` above the last page's final id and `create_date` before the cutoff, ordered by id and limited to `TOKEN_VACUUM_PAGE`, and unlinks each page, so memory stays bounded however many rows exist. An error from search or unlink propagates to the cron runner, which logs it and retries on the next day's run; rows already deleted stay deleted. Special concerns: it only removes rows, never reads or exposes token values, and it takes no lock, so a token used while the cron runs is simply deleted a day later. Test: `[@ANCHOR: test_gdpr_token_vacuum]` in `tests/test_privacy_gdpr.py` (five old tokens over pages of two, one recent token kept).
