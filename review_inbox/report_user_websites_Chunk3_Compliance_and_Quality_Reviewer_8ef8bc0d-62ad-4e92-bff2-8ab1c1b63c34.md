---

## Review Report: user_websites — Compliance_and_Quality_Reviewer

**Reviewer Role:** Compliance_and_Quality_Reviewer
**Module Path:** `/home/bruce/workspace/hams_open/user_websites`
**Files Reviewed:** 8
**Total Findings:** 10

### Summary

The module generally adheres to architectural standards (Zero-Sudo, Odoo 19 constraints, and SRE loop management), but contains several compliance gaps with the DevSecOps pipeline. The most notable issues are the complete absence of SPDX license identifiers, a missing external dependency for fail-fast resolution, and a missing audit-ignore tag for automated test verification of emails.

### Findings

| # | Severity | File | Line | Issue Description | TargetContent | ReplacementContent |
|---|----------|------|------|-------------------|---------------|--------------------|
| 1 | ERROR | `__manifest__.py` | 31 | Missing 'markupsafe' in external_dependencies (ADR-0073). | `        "python": [],` | `        "python": ["markupsafe"],` |
| 2 | INFO | `__manifest__.py` | 1 | Missing SPDX-License-Identifier at top of file. | `{` | `# -*- coding: utf-8 -*-\n# SPDX-License-Identifier: AGPL-3.0-or-later\n{` |
| 3 | INFO | `hooks.py` | 2 | Missing SPDX-License-Identifier. | `# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).` | `# Copyright © Bruce Perens K6BP.\n# SPDX-License-Identifier: AGPL-3.0-or-later` |
| 4 | INFO | `models/__init__.py` | 2 | Missing SPDX-License-Identifier. | `# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).` | `# Copyright © Bruce Perens K6BP.\n# SPDX-License-Identifier: AGPL-3.0-or-later` |
| 5 | INFO | `models/blog_blog.py` | 2 | Missing SPDX-License-Identifier. | `# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).` | `# Copyright © Bruce Perens K6BP.\n# SPDX-License-Identifier: AGPL-3.0-or-later` |
| 6 | INFO | `models/blog_post.py` | 2 | Missing SPDX-License-Identifier. | `# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).` | `# Copyright © Bruce Perens K6BP.\n# SPDX-License-Identifier: AGPL-3.0-or-later` |
| 7 | INFO | `models/res_config_settings.py` | 1 | Missing SPDX-License-Identifier. | `# This software is distributed under the terms of the Affero General Public License (AGPL-3).` | `# SPDX-License-Identifier: AGPL-3.0-or-later` |
| 8 | INFO | `models/res_users.py` | 2 | Missing SPDX-License-Identifier. | `# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).` | `# Copyright © Bruce Perens K6BP.\n# SPDX-License-Identifier: AGPL-3.0-or-later` |
| 9 | INFO | `models/user_websites_owned_mixin.py` | 2 | Missing SPDX-License-Identifier. | `# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).` | `# Copyright © Bruce Perens K6BP.\n# SPDX-License-Identifier: AGPL-3.0-or-later` |
| 10 | ERROR | `models/blog_post.py` | 334 | Missing `# audit-ignore-mail` tag for `send_mail` call which will fail the AST linter. | `template.with_user(mail_svc).with_context(**ctx).send_mail(digest.first_post_id, force_send=False, email_values=email_vals)   # # Verified by [@ANCHOR: COMM_test_weekly_digest_mail_template]  # fmt: skip` | `template.with_user(mail_svc).with_context(**ctx).send_mail(digest.first_post_id, force_send=False, email_values=email_vals)  # audit-ignore-mail: Tested by [@ANCHOR: COMM_test_weekly_digest_mail_template]  # fmt: skip` |

### Areas Reviewed With No Issues

- `models/user_websites_owned_mixin.py` — Proxy ownership constraints, proxy creation, and writes correctly mapped and optimized for Odoo 19.
- `models/res_users.py` — Background job scaling (`_async_unpublish_content`), `website_slug` formatting constraints, and query-efficient batch updates.
- `models/blog_blog.py` — Access rights and constraints properly configured.

---
