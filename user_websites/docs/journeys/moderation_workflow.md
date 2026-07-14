# Journey: Content Reporting and Resolution

This journey describes the workflow for maintaining community standards through reporting and moderation.

## Path: Reporting a Violation

1. **Encounter**: A visitor views a page and finds content that violates community guidelines.
2. **Action**: The visitor clicks the "Report Violation" button in the footer.
3. **Submission**: The visitor fills out the report form and submits it ([@ANCHOR: user_websites:UX_REPORT_VIOLATION]). Verified by `[@ANCHOR: test_tour_violation_report]`.
4. **Validation**: The system performs honeypot checks to block automated spam bots.
5. **Record Creation**: A `content.violation.report` record is created in the backend.

## Path: Administrative Review

1. **Notification**: An administrator sees a toast notification on their next page load ([@ANCHOR: admin_toast_logic]). Verified by `[@ANCHOR: test_tour_toast_notifications]`.

2. **Back-end Check**: The frontend queries the count of pending reports via a dedicated API endpoint ([@ANCHOR: api_pending_reports]). Verified by `[@ANCHOR: test_admin_violation_toast_rpc]`.
3. **Investigation**: The administrator reviews the reported content and the description provided by the reporter.
4. **Action**: The administrator clicks "Take Action and Strike" ([@ANCHOR: action_take_action_and_strike]). Verified by `[@ANCHOR: test_moderation_suspension]`.
5. **Enforcement**:
    - The user receives a strike.
    - If the user exceeds the strike threshold, their `is_suspended_from_websites` flag is set to True.
    - All their content is immediately unpublished ([@ANCHOR: action_take_action_and_strike]). Verified by `[@ANCHOR: test_moderation_suspension]`.

## Path: User Appeal

1. **Observation**: The suspended user logs in and finds they can no longer access their site or creation tools.
2. **Appeal**: The user navigates to their portal and submits an appeal explanation ([@ANCHOR: UX_SUBMIT_APPEAL]). Verified by `[@ANCHOR: test_tour_moderation_appeal]`.
3. **Review**: The administrator reviews the appeal and decides whether to reinstate the user.
