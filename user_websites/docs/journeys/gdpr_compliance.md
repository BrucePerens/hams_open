# Journey: User Data Management (GDPR)

This journey describes how a user manages their data footprint on the platform. Each step that has an anchor is explained in full in the "Anchor reference" at the end of this file (ADR 0113); the entry names are given in square brackets.

## Path: Data Portability

1. **Dashboard**: The user navigates to `/my/privacy` [privacy dashboard].
2. **Request**: The user clicks the "Export My Data" button.
3. **Compilation**: The system executes `_get_gdpr_export_data` [export data].
4. **Streaming**: To handle potentially large amounts of content, the system streams the JSON response to the user's browser via `/my/privacy/export` [export route].
5. **Receipt**: The user receives a comprehensive JSON file containing their site data.
6. **Bulk CSV/Zip Export**: `/my/privacy/export.zip` [zip export route] mints a short-lived, single-use `ham.gdpr.export.token` [export token model] scoped to exactly one user and one narrow time window, then redirects the browser to the export daemon's own download endpoint. The daemon hands the token back to Odoo's `consume_and_export` RPC entrypoint, which validates and consumes it before materializing the export payload, so the daemon itself never needs standing database credentials.

## Path: Account Deletion (Right to Erasure)

1. **Dashboard**: The user navigates to the Privacy Dashboard.
2. **Deletion**: The user clicks "Delete My Content" [erasure route].
3. **Processing**: The system deletes the member's content in batches under a service account [erasure of content]. As written the batches run inside the delete request, not in a separate background executor (to-do: settle which is meant).
4. **Content Removal**: The task unlinks all pages, blog posts, and media associated with the user.
5. **Directory removal**: The user is removed from the public community directory (`privacy_show_in_directory` is turned off). This module does not anonymize the profile; the account steps beyond it belong to the hams.com onboarding module, if installed.
6. **Confirmation**: The user is logged out and redirected to a confirmation page.

## Anchor reference

### Privacy dashboard, step 1 of the portability path [@ANCHOR: user_websites:COMM_privacy_dashboard]

`privacy_dashboard(self, **kwargs)`, `GET /my/privacy` in `controllers/main.py`, signed-in members only (`auth="user"`, `website=True`). It returns the page `user_websites.portal_my_privacy` with an empty context. In this journey it is the starting point of both paths: the "Download My Data" card posts to the export route, the "Right to Erasure" card posts, after a confirm dialog and with a CSRF token, to the erasure route. The page itself reads and changes nothing. Test: `[@ANCHOR: test_tour_gdpr_privacy]`, the browser tour `gdpr_privacy_tour.js` started from `tests/test_ui_tours.py::test_03_gdpr_privacy_tour` as a member, covering export and erasure from this page.

### Export data, step 3 [@ANCHOR: res_users_gdpr_export]

`ResUsers._get_gdpr_export_data(self)` in `models/res_users.py`: the compilation step. `ensure_one()`; starts from the compliance module's base dictionary (`super()`), ensures a `"user"` entry and adds `name`, `email` and `website_slug` (`False` if none). Returns the dictionary; reads only. Bulk content is not in it but in `_get_gdpr_streamed_keys()`, whose generators produce the member's pages, blog posts, submitted reports and appeals a thousand rows at a time, each batch in its own cursor as the service account, so that step 4 can stream them. Test: `[@ANCHOR: test_gdpr_export_hook]` (`tests/test_sdk_extensibility.py::test_01_gdpr_export_hook`): creates a page for a member, then checks the `user` key, the member's name and that `pages` and `blog_posts` are streamed keys.

### Export route, step 4 [@ANCHOR: UX_GDPR_EXPORT]

The "Export Data to JSON" button (help text under `UX_GDPR_EXPORT` in `data/documentation.html`) and the controller `privacy_export(self, **kwargs)`, `/my/privacy/export` [@ANCHOR: user_websites:COMM_privacy_export], `auth="user"`. It composes the export of the signed-in member only and returns a streaming `Response`: one JSON object, the dictionary keys first, then each streamed key as an array filled item by item, so a member with thousands of pages never has to fit in memory. Headers: JSON content type, `Content-Disposition: attachment; filename="gdpr_export.json"`, and `Cache-Control: private, no-store`. No writes, and no argument can select another member's data. The dashboard form posts here, but the route is not limited to POST, so a plain link works too. Test: `[@ANCHOR: test_gdpr_export_api]`, `tests/test_privacy_gdpr.py::test_01_data_portability_export`, which logs in as a member with a page and a post, downloads this route, parses the JSON and checks that the member's name, pages, posts, reports and appeals are present.

### Zip export route, step 6 [@ANCHOR: user_websites:COMM_privacy_export_zip]

`privacy_export_zip(self, **kwargs)`, `GET /my/privacy/export.zip` (`auth="user"`). It does not build the file: it calls `ham.gdpr.export.token.create_for_current_user()` to get a token, answers a redirect to `/api/v1/gdpr_export/download` (the standalone export daemon) and puts the token in a cookie named `gdpr_export_token` (max age 5 minutes, path `/api/v1/gdpr_export/`, `HttpOnly`, `SameSite=Strict`, `Secure` when the request is https), so the token never appears in a URL, a log or a Referer header. Cost on the Odoo worker is only the token insert; the streaming runs in the daemon. Errors: none of its own. Test: `tests/test_privacy_gdpr.py::test_01a_export_zip_route_redirects_with_a_token` (anchor `test_gdpr_export_zip_redirect`) checks the 303 status, the exact `Location` (no query string), the cookie, and that the minted row exists, belongs to the member and is unconsumed.

### Export token model, step 6 [@ANCHOR: gdpr_export_token]

The model `ham.gdpr.export.token` in `models/ham_gdpr_export_token.py`: a short-lived, single-use authorization for one export download. Fields: `user_id` (many2one `res.users`, required, indexed, cascade delete), `token` (char, required, indexed, unique by SQL constraint, not copied), `consumed` (boolean, default False), `name` (computed, stored: "GDPR Export Token for <user>"). Lifetime `TOKEN_EXPIRY_MINUTES` = 5. Only `create_for_current_user()` creates rows (as the dedicated GDPR export service account, never by the member, 32 random url-safe bytes); only `_consume(token)` consumes them, by one atomic `UPDATE ... WHERE consumed = false RETURNING`, so two concurrent holders cannot both win; an unknown, used or expired token raises `GdprExportTokenRejected`, an `AccessError`. `consume_and_export(token)` is the RPC the daemon calls; it is allowed only for the export service account and system administrators. Tests in `tests/test_privacy_gdpr.py`: single use, unknown rejected, expiry, consume-and-export payload.

### Erasure route, step 2 of the erasure path [@ANCHOR: UX_GDPR_ERASURE]

The red "Erase My Account & Content" button (help text `UX_GDPR_ERASURE` in `data/documentation.html`) and the controller `privacy_delete_content(self, **kwargs)`, `POST /my/privacy/delete_content` [@ANCHOR: user_websites:COMM_privacy_delete_content], `auth="user"`, `csrf=True`. It takes the user from the session, calls `user._execute_gdpr_erasure()` and redirects to the public `/privacy/erased` page, because the account is now deactivated and any `auth="user"` page would show a login form instead of the result. Irreversible; the dashboard warns the member to export first. Failures in erasure propagate as a server error and leave already-committed batches in place, so the member can press the button again. The redirect response is followed by the test client to the confirmation page, which is public and takes no input. Test: `[@ANCHOR: test_gdpr_erasure_pages]`, `tests/test_privacy_gdpr.py`, which posts a member with a page and a post to the route with a CSRF token, checks the landing page is `/privacy/erased` with the text "Your data has been erased", that the page and post no longer exist, that the account is deactivated and that the directory flag is off.

### Erasure of content, steps 3 to 5 [@ANCHOR: gdpr_sudo_erasure]

`ResUsers._execute_gdpr_erasure(self)` in `models/res_users.py`. No arguments, `ensure_one()`, returns nothing. Runs as `zero_sudo.gdpr_service_internal` for search and delete alike, so any caller works. It deletes the member's `website.page`, `blog.post` and `blog.blog` records in loops of 5000 per savepoint, commits after each batch (not in tests), sleeps 0.1 s between full batches (skipped when `ODOO_DISABLE_SLEEPS` is set), retries after 0.5 s on a concurrent-update, serialization or deadlock error and re-raises other errors, then removes up to 500 site-editor images, turns `privacy_show_in_directory` off and calls `super()` for the compliance and hams.com steps. Tests: `test_gdpr_erasure_pages` (shared with the route entry above) and `test_gdpr_erasure_posts`, both in `tests/test_privacy_gdpr.py`.
