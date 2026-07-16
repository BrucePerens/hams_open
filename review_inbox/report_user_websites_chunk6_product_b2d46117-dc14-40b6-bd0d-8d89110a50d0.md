# Sub-Agent Review Report Template

---

## Review Report: user_websites — Product_and_UX_Reviewer

**Reviewer Role:** Product_and_UX_Reviewer
**Module Path:** `/home/bruce/workspace/hams_open/user_websites`
**Files Reviewed:** 10
**Total Findings:** 10

### Summary

The `user_websites` module views are well-structured and properly implement the UI constraints, such as standardizing `invisible="not id"` for backend action buttons to avoid test races. However, there are multiple violations of ADR 0081 in the frontend templates involving missing `o_tour_` hooks for generic interactive elements, and a banned `min-height: 1px` hack in the backend blog layout view.

### Findings

| # | Severity | File | Line | Issue Description | TargetContent | ReplacementContent |
|---|----------|------|------|-------------------|---------------|--------------------|
| 1 | ERROR | `views/blog_post_views.xml` | 17 | ADR 0081 prohibits `min-height: 1px` hacks for structural elements to be targeted by tests. | `<div id="user_websites_dropzone_blog_post" style="min-height: 1px;">` | `<div id="user_websites_dropzone_blog_post">` |
| 2 | ERROR | `views/user_websites_templates.xml` | 107 | ADR 0081 requires QWeb frontend buttons without a native name/id to include an `o_tour_` class. | `<button type="submit" class="btn btn-danger">Submit Appeal</button>` | `<button type="submit" class="btn btn-danger o_tour_submit_appeal_btn">Submit Appeal</button>` |
| 3 | ERROR | `views/user_websites_templates.xml` | 133 | ADR 0081 requires QWeb frontend buttons without a native name/id to include an `o_tour_` class. | `<button type="submit" class="btn btn-danger">Submit Appeal</button>` | `<button type="submit" class="btn btn-danger o_tour_submit_group_appeal_btn">Submit Appeal</button>` |
| 4 | WARNING | `views/user_websites_templates.xml` | 176 | ADR 0081 requires QWeb frontend buttons without a native name/id to include an `o_tour_` class. | `<button type="submit" class="btn btn-outline-primary"><i class="fa fa-download" aria-hidden="true"/> Export Data to JSON</button>` | `<button type="submit" class="btn btn-outline-primary o_tour_export_data_btn"><i class="fa fa-download" aria-hidden="true"/> Export Data to JSON</button>` |
| 5 | WARNING | `views/user_websites_templates.xml` | 234 | ADR 0081 requires QWeb frontend anchors functioning as buttons to include an `o_tour_` class. | `<a t-attf-href="/blog/#{post.blog_id.id}/post/#{post.id}" class="btn btn-outline-primary mt-2">Read Post</a>` | `<a t-attf-href="/blog/#{post.blog_id.id}/post/#{post.id}" class="btn btn-outline-primary mt-2 o_tour_read_post_btn">Read Post</a>` |
| 6 | WARNING | `views/user_websites_templates.xml` | 274 | ADR 0081 requires QWeb frontend anchors functioning as buttons to include an `o_tour_` class. | `<a t-attf-href="/#{entry.website_slug}/home" class="btn btn-primary py-2 fw-bold">` | `<a t-attf-href="/#{entry.website_slug}/home" class="btn btn-primary py-2 fw-bold o_tour_visit_home_btn">` |
| 7 | WARNING | `views/user_websites_templates.xml` | 277 | ADR 0081 requires QWeb frontend anchors functioning as buttons to include an `o_tour_` class. | `<a t-attf-href="/#{entry.website_slug}/blog" class="btn btn-outline-secondary py-2 fw-bold">` | `<a t-attf-href="/#{entry.website_slug}/blog" class="btn btn-outline-secondary py-2 fw-bold o_tour_view_blog_btn">` |
| 8 | ERROR | `views/user_websites_templates.xml` | 335 | ADR 0081 requires QWeb frontend buttons without a native name/id to include an `o_tour_` class. | `<button type="submit" class="btn btn-danger">Submit Report</button>` | `<button type="submit" class="btn btn-danger o_tour_submit_report_btn">Submit Report</button>` |
| 9 | WARNING | `views/user_websites_templates.xml` | 347 | ADR 0081 requires QWeb frontend buttons without a native name/id to include an `o_tour_` class. | `<button type="button" class="btn btn-outline-danger btn-sm" data-bs-toggle="modal" data-bs-target="#reportViolationModal" t-att-data-url="request.httprequest.url">` | `<button type="button" class="btn btn-outline-danger btn-sm o_tour_report_violation_btn" data-bs-toggle="modal" data-bs-target="#reportViolationModal" t-att-data-url="request.httprequest.url">` |
| 10| WARNING | `views/user_websites_templates.xml` | 366 | ADR 0081 requires QWeb frontend anchors functioning as buttons to include an `o_tour_` class. | `<a href="/" class="btn btn-primary mt-4">Return to Homepage</a>` | `<a href="/" class="btn btn-primary mt-4 o_tour_return_home_btn">Return to Homepage</a>` |

### Areas Reviewed With No Issues

- `views/content_violation_appeal_views.xml` — standard backend buttons properly use `invisible="not id"` for tours.
- `views/content_violation_report_views.xml` — standard backend buttons properly use `invisible="not id"` for tours.
- `views/res_config_settings_views.xml` — clean inheritance.
- `views/res_users_views.xml` — clean backend configuration and tour safety implementations.
- `views/snippets.xml` — clean QWeb XML structures.
- `views/user_websites_group_views.xml` — backend buttons properly implemented with `invisible="not id"`.
- `views/website_layout.xml` — valid QWeb xpath insertions and variables set up.
- `views/website_page_views.xml` — clean XML views.

---
