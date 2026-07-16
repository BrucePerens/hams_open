---

## Review Report: user_websites — Product_and_UX_Reviewer

**Reviewer Role:** Product_and_UX_Reviewer
**Module Path:** `hams_open/user_websites`
**Files Reviewed:** 10
**Total Findings:** 8

### Summary

The module is structurally sound and effectively adheres to the Zero-Sudo architecture, leveraging Service Accounts and stored procedures safely. However, a CRITICAL violation was identified regarding un-savepointed raw SQL executions during `_auto_init` via `init()` methods, which poses a severe risk to test runner and registry stability as per the Project Experience Trap 18. I have also identified several minor semantic tag anomalies where `# # Tested by` was used improperly instead of `# Verified by`.

### Findings

| # | Severity | File | Line | Issue Description | TargetContent | ReplacementContent |
|---|----------|------|------|-------------------|---------------|--------------------|
| 1 | CRITICAL | `models/sql_views.py` | 19 | Missing `savepoint()` around raw SQL execution during `_auto_init` which crashes registry init if it fails (Trap 18). | `        self.env.cr.execute(\n            """\n            CREATE OR REPLACE VIEW user_websites_public_directory_view AS (` | `        with self.env.cr.savepoint():\n            self.env.cr.execute(\n                """\n                CREATE OR REPLACE VIEW user_websites_public_directory_view AS (` |
| 2 | CRITICAL | `models/sql_views.py` | 53 | Missing `savepoint()` around raw SQL execution during `_auto_init` which crashes registry init if it fails (Trap 18). | `        self.env.cr.execute(\n            """\n            CREATE OR REPLACE VIEW user_websites_content_routing_view AS (` | `        with self.env.cr.savepoint():\n            self.env.cr.execute(\n                """\n                CREATE OR REPLACE VIEW user_websites_content_routing_view AS (` |
| 3 | CRITICAL | `models/sql_views.py` | 96 | Missing `savepoint()` around raw SQL execution during `_auto_init` which crashes registry init if it fails (Trap 18). | `        self.env.cr.execute(\n            """\n            CREATE OR REPLACE VIEW user_websites_weekly_digest_view AS (` | `        with self.env.cr.savepoint():\n            self.env.cr.execute(\n                """\n                CREATE OR REPLACE VIEW user_websites_weekly_digest_view AS (` |
| 4 | CRITICAL | `models/sql_views.py` | 144 | Missing `savepoint()` around raw SQL execution during `_auto_init` which crashes registry init if it fails (Trap 18). | `        self.env.cr.execute(\n            """\n            CREATE OR REPLACE FUNCTION increment_strike_count(tbl_name text, rec_id integer)` | `        with self.env.cr.savepoint():\n            self.env.cr.execute(\n                """\n                CREATE OR REPLACE FUNCTION increment_strike_count(tbl_name text, rec_id integer)` |
| 5 | ERROR | `models/content_violation_appeal.py` | 60 | Incorrect semantic traceability tag ("Tested by" used instead of "Verified by" in source code) and duplicate hash. | `    def action_approve(self):\n        # # Tested by [@ANCHOR: user_websites:test_tour_moderation_appeal]` | `    def action_approve(self):\n        # Verified by [@ANCHOR: user_websites:test_tour_moderation_appeal]` |
| 6 | INFO | `models/content_violation_report.py` | 64 | Minor formatting issue with duplicate comment hashes. | `        # # Verified by [@ANCHOR: test_cron_pending_reports]\n\n        # # Verified by [@ANCHOR: COMM_test_cron_pending_reports]` | `        # Verified by [@ANCHOR: test_cron_pending_reports]\n\n        # Verified by [@ANCHOR: COMM_test_cron_pending_reports]` |
| 7 | INFO | `models/content_violation_report.py` | 88 | Minor formatting issue with linter bypass tag (extra hash breaks regex parsing). | `                template.with_user(mail_svc).with_context(pending_count=count).send_mail(self.env.company.id, force_send=False, email_values=email_vals)  # audit-ignore-mail: # Tested by [@ANCHOR: test_cron_pending_reports]  # fmt: skip` | `                template.with_user(mail_svc).with_context(pending_count=count).send_mail(self.env.company.id, force_send=False, email_values=email_vals)  # audit-ignore-mail: Tested by [@ANCHOR: test_cron_pending_reports]  # fmt: skip` |
| 8 | INFO | `models/content_violation_report.py` | 100 | Minor formatting issue with duplicate comment hashes. | `    def action_take_action_and_strike(self):\n        # [@ANCHOR: action_take_action_and_strike]\n\n        # # Verified by [@ANCHOR: test_moderation_suspension]` | `    def action_take_action_and_strike(self):\n        # [@ANCHOR: action_take_action_and_strike]\n\n        # Verified by [@ANCHOR: test_moderation_suspension]` |

### Areas Reviewed With No Issues

- `__init__.py`, `__manifest__.py`, `hooks.py`, `models/__init__.py` — Clean initialization, metadata, and routing. Uses exact parameterized values and implements `savepoint()` correctly in `hooks.py`.
- `models/blog_blog.py` — Excellent use of ORM `models.Constraint` and strictly follows the Service Account pattern for access verification without using `sudo()`. Pre-fetches relational data to avoid N+1 issues in access checks.
- `models/res_config_settings.py` — Properly uses `with_user()` to bypass access restrictions when mapping admin groups safely.
- `models/res_users_moderation.py` — Thread-safe asynchronous background content suspension without holding up the WSGI thread, avoiding UI blocking. Reliable batch limiters implemented in tests.
