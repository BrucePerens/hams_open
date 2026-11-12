# Journey: Content Reporting and Resolution

This journey describes the workflow for maintaining community standards through reporting and moderation. Each step that has an anchor is explained in full in the "Anchor reference" at the end of this file (ADR 0113); the entry names are given in square brackets.

## Path: Reporting a Violation

1. **Encounter**: A visitor views a page and finds content that violates community guidelines.
2. **Action**: The visitor clicks the "Report Violation" button in the footer.
3. **Submission**: The visitor fills out the report form and submits it [report submission].
4. **Validation**: The system performs honeypot checks to block automated spam bots.
5. **Record Creation**: A `content.violation.report` record is created in the backend.

## Path: Administrative Review

1. **Notification**: An administrator sees a toast notification on their next page load [pending-moderation toast].
2. **Back-end Check**: The frontend queries the count of pending reports via a dedicated API endpoint [pending-reports endpoint].
3. **Investigation**: The administrator reviews the reported content and the description provided by the reporter.
4. **Action**: The administrator clicks "Take Action and Strike" [strike action].
5. **Enforcement**:
    - The user receives a strike.
    - If the user exceeds the strike threshold, their `is_suspended_from_websites` flag is set to True.
    - All their content is immediately unpublished [strike action].
    - When an entire `user.websites.group` is suspended rather than a single user, unpublishing its content runs in a background thread (`_async_unpublish_group_content`) instead of inline, to avoid holding a transaction lock open for however long a large group's content takes to unpublish. Because that thread's `Future` is never awaited, two failure modes are guarded explicitly rather than allowed to vanish silently: a failure acquiring the registry/cursor itself [registry failure guard], and any other unexpected, non-database exception raised while unpublishing [catch-all guard]. Both are logged at ERROR level and the thread returns cleanly instead of crashing with no trace.

## Path: User Appeal

1. **Observation**: The suspended user logs in and finds they can no longer access their site or creation tools.
2. **Appeal**: The user navigates to their portal and submits an appeal explanation [appeal submission].
3. **Review**: The administrator reviews the appeal and decides whether to reinstate the user.

## Anchor reference

### Report submission, step 3 of the reporting path [@ANCHOR: user_websites:UX_REPORT_VIOLATION]

The controller `report_violation(self, url="", reason="", description="", email="", website_honeypot="", **post)`, `POST /website/report_violation` in `controllers/main.py`, public, CSRF-checked. In this journey it is the step where the visitor's form becomes a record. The order is: a filled honeypot field is discarded silently with the normal success redirect; a missing `url` falls back to the `Referer` header; an empty URL or description redirects with `report_error=missing_fields`; the `form_shield` module then checks proof of work or question, time-to-submit and rate limits; the structural checks (`report_problem`) refuse short, random or link-heavy guest reports; the description is cut to 5000 characters; finally one `content.violation.report` row is created in state `new` through the service account, with the owner or group taken from the first path segment of the URL and the reporter recorded from the session when signed in. The visitor is always redirected back to the reported path with `report_submitted=1` or `report_error=...`. Test: `[@ANCHOR: test_tour_violation_report]`, the browser tour that files a report as a guest from a normal member page and checks the stored row has the typed email address and the page owner (`tests/test_ui_tours.py`).

### Pending-moderation toast, step 1 of the review path [@ANCHOR: admin_toast_logic]

The front-end method `AdminViolationToast._checkPendingReports()` in `static/src/js/toast_notifications.js`, started on every page load by `start()`. It fetches the endpoint below, then shows a sticky warning toast "Pending Moderation" with "Review reports" (a link to the pending queue, only when the server's URL is a path starting with `/`) and "Dismiss" buttons, unless `shouldShowToast` says it was already shown: not within 12 hours for the same or a smaller queue, always when the count or the newest report id grew. The sighting is remembered in `localStorage` under `admin_violation_toast_seen`; if storage is blocked the toast simply repeats. Network errors are swallowed so a failure never disturbs the page. Returns a promise, no arguments. Test: `[@ANCHOR: test_tour_toast_notifications]`, the tour `toast_notifications_tour.js` (`tests/test_ui_tours.py`).

### Pending-reports endpoint, step 2 of the review path [@ANCHOR: user_websites:COMM_pending_reports]

`pending_reports(self, **kwargs)`, `GET /api/v1/user_websites/pending_reports` in `controllers/main.py` (`auth="public"`, `sitemap=False`). Anyone who is not a user-websites administrator or system administrator gets status 200 with `{"count": 0, "error": "Forbidden"}`; an administrator gets `{"count": <reports in state new, at most 1000>, "latest_id": <id of the newest, or 0>, "url": "<back-office pending queue>"}`, counted through the module's service account. Read only; the caller is the toast above, which compares `count` and `latest_id` with what it showed last. Test: `[@ANCHOR: test_admin_violation_toast_rpc]` (`tests/test_controllers.py::test_01_api_pending_reports_admin_access`), which logs in as an administrator and checks status 200 and a `count` of at least 1.

### Strike action, steps 4 and 5 of the review path [@ANCHOR: action_take_action_and_strike]

The "Take Action and Strike" button method on `content.violation.report`, owned by the `content_moderation` module and extended by this module's `_apply_enforcement_action()` override in `models/content_violation_report_moderation.py`. It refuses a report already resolved; otherwise it sets the report to `action_taken` and applies the strike: the content owner's strike count is incremented atomically by the stored procedure `increment_strike_count()` (a row lock), at 3 strikes the owner is suspended (`is_suspended_from_websites`), which unpublishes all their content in the background after commit, and a chatter note records the new count. For a group the group's count and `action_suspend_group_websites` are used. The same method is called automatically when the page sanitizer sees an injection attempt. Test: `[@ANCHOR: test_moderation_suspension]` (`tests/test_moderation.py::test_01_three_strikes_suspension`), which strikes three times and checks suspension and unpublishing.

### Registry failure guard [@ANCHOR: COMM_user_websites_async_unpublish_registry_failure]

The first `try/except` of `_async_unpublish_group_content(db_name, group_ids)` in `models/user_websites_groups.py`, the background worker for group suspension (submitted to a 4-thread executor after the suspending transaction commits). It wraps `Registry(db_name)` and `registry.cursor()`. If either raises, for example for a database name that does not exist, the guard logs "Async unpublish group content failed to acquire a registry/cursor" at ERROR level with the traceback and returns, with nothing unpublished. Before the guard existed the thread died with no log at all, indistinguishable from never having been scheduled. Returns `None`; the caller never sees the failure, so the log line is the only signal. Test: `tests/test_audit_edge_cases.py::test_async_unpublish_survives_registry_acquisition_failure`, which passes an invalid database name and checks the log line.

### Catch-all guard [@ANCHOR: COMM_user_websites_async_unpublish_catch_all]

The last `except Exception` of `_async_unpublish_group_content(db_name, group_ids)`. Database errors (`psycopg2.Error`) are logged by their own handler; any other exception raised while looking up the service account, enumerating groups or unpublishing is logged with its traceback as "Async unpublish group content crashed unexpectedly" and the function returns normally, and the cursor is always closed in a `finally`. The thread's `Future` is never awaited, so without this the exception would vanish and group suspension would silently not unpublish anything, as happened for a while with a bad `with_company` call. A failure of one company is also isolated so other companies in the batch are still processed. Test: `tests/test_audit_edge_cases.py::test_async_unpublish_survives_non_psycopg2_exception`, which patches `_get_service_env` to raise `TypeError` and checks the log line.

### Appeal submission, step 2 of the appeal path [@ANCHOR: UX_SUBMIT_APPEAL]

`submit_appeal(self, reason="", group_id=None, **post)`, `POST /website/submit_appeal` in `controllers/main.py` (`auth="user"`, CSRF checked). A suspended member submits `reason` (required, else redirect `/my/home?error=missing_reason`) and optionally `group_id` (an appeal for a suspended group; the member must belong to it, else `error=appeal_failed`). It creates one `content.violation.appeal` in state `new` through the service account and redirects to `/my/home?appeal_submitted=1`, shown as a "Submitted" toast. `KeyError` or `ValueError` while creating are logged and answer `appeal_failed`. Deciding the appeal and reinstating the user is a manual administrator step in the back office (step 3 of the path). Test: `[@ANCHOR: test_tour_moderation_appeal]`, the tour `static/tests/tours/moderation_appeal_tour.js` that fills the suspension alert's appeal form and submits it.
