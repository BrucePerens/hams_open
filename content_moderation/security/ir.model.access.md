### Explanation of `ir.model.access.csv`

This CSV file is a critical part of Odoo's security system. It defines the base access control rights for various user groups on different data models. Each row in this file grants or denies permissions (Read, Write, Create, Delete) to a specific security group for a specific model.

Here's a breakdown of the rules in this file:

- **`access_content_violation_report_mail_svc`**
  - **Group:** `zero_sudo.group_mail_service`
  - **Model:** `content.violation.report`
  - **Permissions:** Read, Write (no Create/Delete).
  - **Purpose:** Lets the shared mail-service account post chatter messages (state-change notes, enforcement audit trail) onto a report on a moderator's behalf, without granting it full moderator access.

- **`access_content_violation_report_moderator`**
  - **Group:** Moderator (`group_content_moderation_moderator`)
  - **Model:** `content.violation.report`
  - **Permissions:** Full access (Read, Write, Create, Delete).
  - **Purpose:** Allows this module's own moderator role to manage every report, platform-wide, regardless of company. Extracted 2026-09-23 from `user_websites`' own `access_content_violation_report_admin`; `user_websites.group_user_websites_administrator` is wired into this group via `implied_ids` (see `user_websites/security/user_websites_security.xml`'s `noupdate="0"` block), so an existing Administrator's access is unchanged by the extraction.

- **`access_content_violation_report_user`**
  - **Group:** `base.group_portal` (any logged-in portal user)
  - **Model:** `content.violation.report`
  - **Permissions:** Read, Create (no Write/Delete).
  - **Purpose:** Allows any logged-in user to submit a new report and view their own past reports (further scoped by `content_violation_report_user_rule`'s own `ir.rule`), but not to edit or delete them.

- **`access_content_violation_report_public`**
  - **Group:** `base.group_public` (not logged in)
  - **Model:** `content.violation.report`
  - **Permissions:** Create only.
  - **Purpose:** Allows non-logged-in users (guests) to submit new reports.

- **`access_content_violation_report_svc`**
  - **Group:** Service Account: Content Moderation (`group_content_moderation_service_account`)
  - **Model:** `content.violation.report`
  - **Permissions:** Full access (Read, Write, Create, Delete), scoped to its own company by `content_violation_report_service_account_rule` (deliberately NOT the moderator's own unconditional rule -- see that rule's own comment in `security/content_moderation_security.xml` for why sharing it would leak reports across companies).
  - **Purpose:** Lets a consuming module's own service/automation account (e.g. a public-submission controller resolving and creating a report) read and write reports without holding the full human-moderator role.
