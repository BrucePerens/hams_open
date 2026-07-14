# Story: Privacy and GDPR Compliance

As a **Community Member**, I want full control over my personal data so that I can exercise my "Right to be Forgotten" and "Right to Portability" as required by GDPR.

## Scenarios

### Exporting Personal Data
- **Given** I am logged into my account
- **When** I visit my Privacy Dashboard ([@ANCHOR: controller_my_privacy_dashboard])

- **Then** I can request a full export of my data ([@ANCHOR: UX_GDPR_EXPORT]). Verified by `[@ANCHOR: test_tour_gdpr_privacy]`.

- **And** the system generates a machine-readable JSON file containing all my pages, blog posts, and metadata ([@ANCHOR: res_users_gdpr_export]). Verified by `[@ANCHOR: test_gdpr_export_hook]`.

### Exercising the Right to Erasure
- **Given** I wish to leave the platform and delete my presence
- **When** I click "Delete My Content" on the Privacy Dashboard ([@ANCHOR: UX_GDPR_ERASURE])

- **Then** the system schedules a background process to permanently delete all my hosted content ([@ANCHOR: gdpr_sudo_erasure]). Verified by `[@ANCHOR: test_gdpr_erasure_pages]`.
- **And** my account is anonymized and deactivated to prevent future data processing.

## Technical Notes
- Data exports use streaming generators to handle large datasets without exhausting server memory ([@ANCHOR: UX_GDPR_EXPORT]).
- Erasure is performed as a background task to ensure a responsive user experience even for users with massive amounts of content.
