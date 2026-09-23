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
- **Then** I can issue a strike ([@ANCHOR: content_moderation:action_take_action_and_strike], moved to the generic `content_moderation` module 2026-09-23; this module's own `_apply_enforcement_action()` override, [@ANCHOR: user_websites:COMM_apply_enforcement_action], is what actually strikes and, past 3 strikes, suspends), which may lead to automatic account suspension. Verified by `[@ANCHOR: test_moderation_suspension]`.

### Automated Security Enforcement
- **Given** a user attempts to save a page with malicious code (SSTI/XSS)
- **When** the system sanitizes the architecture ([@ANCHOR: website_page_sanitize_arch])

- **Then** it automatically triggers a security violation report ([@ANCHOR: content_moderation:action_take_action_and_strike]) and strikes the user's account for attempting to bypass security. Verified by `[@ANCHOR: test_moderation_suspension]`.

### Appealing a Suspension
- **Given** my account has been suspended from the websites platform
- **When** I visit my portal
- **Then** I can submit a moderation appeal ([@ANCHOR: UX_SUBMIT_APPEAL]) to explain my case to the administrators. Verified by `[@ANCHOR: test_tour_moderation_appeal]`.

## Technical Notes
- The "take action" consequence is an extension hook ([@ANCHOR: content_moderation:apply_enforcement_action]), owned by the generic `content_moderation` module: its own default is a documented no-op (no strike infrastructure exists to assume outside a consuming module), and this module's own override ([@ANCHOR: user_websites:COMM_apply_enforcement_action]) is what supplies the real strike-and-suspend behavior described above. Verified by `[@ANCHOR: test_moderation_suspension]`.

- Pending reports are checked via a background RPC call ([@ANCHOR: api_pending_reports]). Verified by `[@ANCHOR: test_admin_violation_toast_rpc]`.

- Administrators receive periodic email notifications about outstanding reports ([@ANCHOR: cron_notify_pending_reports]). Verified by `[@ANCHOR: test_cron_pending_reports]`.
