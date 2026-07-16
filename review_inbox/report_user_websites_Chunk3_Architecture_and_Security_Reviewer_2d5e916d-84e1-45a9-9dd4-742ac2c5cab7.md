---

## Review Report: user_websites — Architecture_and_Security_Reviewer

**Reviewer Role:** Architecture_and_Security_Reviewer
**Module Path:** `/home/bruce/workspace/hams_open/user_websites`
**Files Reviewed:** 8
**Total Findings:** 2 CRITICAL, 2 ERROR, 1 WARNING

### Summary

The core logic handles the Zero Sudo requirements and access controls well, with good batch processing for GDPR deletion and proxy ownership assignment. However, there are critical vulnerabilities in the blog post module involving an XSS via email and an Insecure Direct Object Reference (IDOR) that allows users to reassign their posts to arbitrary blogs, as well as spoof their view counts. Additionally, an implicit missing dependency on `cloudflare` causes a runtime `KeyError`.

### Findings

| # | Severity | File | Line | Issue Description | TargetContent | ReplacementContent |
|---|----------|------|------|-------------------|---------------|--------------------|
| 1 | ERROR | `user_websites/__manifest__.py` | 28 | Missing `cloudflare` module in `depends` while it is required for cache purging, resulting in `KeyError` when missing. | `        "knowledge",\n        "compliance",\n    ],` | `        "knowledge",\n        "compliance",\n        "cloudflare",\n    ],` |
| 2 | CRITICAL | `user_websites/models/blog_post.py` | 316 | Stored Cross-Site Scripting (XSS) / HTML Injection vulnerability in `send_weekly_digest`. The blog post title (`p.name`) is user-supplied and injected directly into an f-string, bypassing `Markup` sanitization. | `            post_links_html = "".join(\n                f"<li><a href='{base_url}{p.website_url}'>{p.name}</a></li>"\n                for p in posts\n            )` | `            post_links_html = "".join(\n                Markup("<li><a href='{}'>{}</a></li>").format(f"{base_url}{p.website_url}", p.name)\n                for p in posts\n            )` |
| 3 | ERROR | `user_websites/models/blog_post.py` | 95 | Data Integrity Violation: `view_count` is included in the `allowed` field dictionary for non-admins in `create`, allowing users to arbitrarily inflate their post's view metrics. | `                "blog_id",\n                "website_id",\n                "view_count",\n                "website_meta_title",` | `                "blog_id",\n                "website_id",\n                "website_meta_title",` |
| 4 | ERROR | `user_websites/models/blog_post.py` | 184 | Data Integrity Violation: `view_count` is included in the `allowed` field dictionary for non-admins in `write`. | `                "blog_id",\n                "website_id",\n                "view_count",\n                "website_meta_title",` | `                "blog_id",\n                "website_id",\n                "website_meta_title",` |
| 5 | CRITICAL | `user_websites/models/blog_post.py` | 102 | Insecure Direct Object Reference (IDOR): No authorization check for `blog_id` in `create`, allowing non-admins to link their posts to blogs they do not own because the write uses a zero_sudo service account bypass. | `            for vals in vals_list:\n                for k in list(vals.keys()):\n                    if k not in allowed:\n                        del vals[k]` | `            for vals in vals_list:\n                for k in list(vals.keys()):\n                    if k not in allowed:\n                        del vals[k]\n                if vals.get("blog_id"):\n                    self.env["blog.blog"].browse(vals["blog_id"]).check_access("write")` |
| 6 | CRITICAL | `user_websites/models/blog_post.py` | 190 | Insecure Direct Object Reference (IDOR): No authorization check for `blog_id` in `write`. | `            for k in list(vals.keys()):\n                if k not in allowed:\n                    del vals[k]` | `            for k in list(vals.keys()):\n                if k not in allowed:\n                    del vals[k]\n            if vals.get("blog_id"):\n                self.env["blog.blog"].browse(vals["blog_id"]).check_access("write")` |
| 7 | WARNING | `user_websites/models/res_users.py` | 485 | Performance Issue: `_get_gdpr_streamed_keys` uses `offset` pagination (`offset += 1000`) in a loop for database queries. While acceptable for a single user's scale, offset pagination operates in O(N^2) complexity. Keysets (e.g. `id > last_id`) would prevent database drag. | `[MANUAL]` | `[MANUAL]` |

### Areas Reviewed With No Issues

- `user_websites/hooks.py` — Database initialization queries use properly parameterized `ON CONFLICT DO NOTHING` SQL.
- `user_websites/models/__init__.py` — Clean.
- `user_websites/models/blog_blog.py` — Zero sudo proxy operations properly manage ACL delegations for writes and deletes.
- `user_websites/models/res_config_settings.py` — Safe mapping for administrators into group assignments.
- `user_websites/models/user_websites_owned_mixin.py` — Enforces ownership constraints appropriately and strictly handles group memberships.
