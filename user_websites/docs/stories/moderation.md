# Story: Content Moderation

As a **Site Visitor** or **Administrator**, I want a way to report and manage content violations to ensure the community remains safe and respectful.

## Scenarios

### Reporting a Violation
- **Given** I am viewing a community member's page
- **When** I encounter inappropriate content
- **Then** I can click the "Report Violation" button ([@ANCHOR: user_websites:UX_REPORT_VIOLATION]) and provide details about the issue. Verified by `[@ANCHOR: test_tour_violation_report]`.
- **And** the system records my report and notifies administrators.

### Reviewing Abuse Reports
- **Given** I am a **User Websites Administrator**
- **When** I log in, I see a toast notification if there are pending reports ([@ANCHOR: admin_toast_logic]). Verified by `[@ANCHOR: test_tour_toast_notifications]`.
- **When** I review a report, I can take action against the content owner.
- **Then** I can issue a strike ([@ANCHOR: action_take_action_and_strike]), which may lead to automatic account suspension. Verified by `[@ANCHOR: test_moderation_suspension]`.

### Automated Security Enforcement
- **Given** a user attempts to save a page with malicious code (SSTI/XSS)
- **When** the system sanitizes the architecture ([@ANCHOR: website_page_sanitize_arch])

- **Then** it automatically triggers a security violation report ([@ANCHOR: action_take_action_and_strike]) and strikes the user's account for attempting to bypass security. Verified by `[@ANCHOR: test_moderation_suspension]`.

### Appealing a Suspension
- **Given** my account has been suspended from the websites platform
- **When** I visit my portal
- **Then** I can submit a moderation appeal ([@ANCHOR: UX_SUBMIT_APPEAL]) to explain my case to the administrators. Verified by `[@ANCHOR: test_tour_moderation_appeal]`.

## Technical Notes
- Pending reports are checked via a background RPC call ([@ANCHOR: api_pending_reports]). Verified by `[@ANCHOR: test_admin_violation_toast_rpc]`.

- Administrators receive periodic email notifications about outstanding reports ([@ANCHOR: cron_notify_pending_reports]). Verified by `[@ANCHOR: test_cron_pending_reports]`.
