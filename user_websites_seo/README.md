# User Websites SEO

This module allows users to optimize their blog pages for search engines (like Google) directly from the website.

## Overview
We integrate search engine optimization (SEO) tools into personal and group blogs. When you visit your blog, you can use the "Optimize SEO" tool to change how your page appears in search results.

## Key Features
- **Easy SEO Management:** Change your blog's title and description without needing technical skills.
- **Social Media Previews:** Set a custom image that appears when your blog is shared on social media.
- **Group Collaboration:** Members of a group can work together to improve their shared blog's visibility.
- **Secure & Private:** Our "Zero-Sudo" security ensures that you can only edit SEO data for your own blog or your groups.

# Technical Documentation

<system_role>
**Context:** Technical documentation for Integrators.
</system_role>

## 1. Architecture
This module extends `user_websites` by adding SEO metadata to users and groups.

## 2. Implementation Details
* **Model Extension:** Inherits `website.seo.metadata` into `res.users` and `user.websites.group`.
* **Access Control:** Whitelists SEO fields in `SELF_WRITEABLE_FIELDS` so users can edit their own metadata. `[@ANCHOR: res_users_self_writeable_fields]`
* **Controller:** Overrides the blog index to set the `main_object`. This enables the frontend SEO widget. `[@ANCHOR: controller_user_blog_index_seo_override]`
* **Security (Zero-Sudo):** Uses service accounts instead of `.sudo()` for database writes.
    * User: `[@ANCHOR: res_users_seo_write_elevation]`
    * Group: `[@ANCHOR: user_websites_group_seo_write_elevation]`
* **SSTI Mitigation:** Controllers ensure recordsets are not elevated when passed to the template engine.

---

<stories_and_journeys>
## 3. Architectural Stories & Journeys

For detailed narratives and end-to-end workflows, refer to the following:

### Stories
* [Individual SEO Control](user_websites_seo/docs/stories/individual_seo_control.md)
* [Group SEO Collaboration](user_websites_seo/docs/stories/group_seo_collaboration.md)
* [Seamless Documentation](user_websites_seo/docs/stories/seamless_documentation.md)

### Journeys
* [User Optimizes Blog SEO](user_websites_seo/docs/journeys/user_optimizes_blog_seo.md)
</stories_and_journeys>

## 4. 🔗 Semantic Anchors & Traceability

| Anchor | Description | Verified By |
|--------|-------------|-------------|
| `[@ANCHOR: res_users_self_writeable_fields]` | Whitelisting SEO fields for users. | `test_self_writeable_fields` |
| `[@ANCHOR: res_users_seo_write_elevation]` | Elevated write for user SEO metadata. | `test_check_access_rule_res_users` |
| `[@ANCHOR: user_websites_group_seo_write_elevation]` | Elevated write for group SEO metadata. | `test_check_access_rule_user_websites_group` |
| `[@ANCHOR: controller_user_blog_index_seo_override]` | Controller override for SEO widget activation. | `test_controller_no_ssti_elevation` |
| `[@ANCHOR: soft_dependency_docs_installation]` | Automatic documentation installation. | `test_soft_dependency_docs_installation` |
| `[@ANCHOR: test_seo_widget_tour]` | UI tour for SEO optimization. | `test_seo_widget_tour` |
| `[@ANCHOR: test_xpath_rendering_res_users]` | Backend view rendering for users. | `test_xpath_rendering_res_users` |
| `[@ANCHOR: test_xpath_rendering_user_websites_group]` | Backend view rendering for groups. | `test_xpath_rendering_user_websites_group` |

## 5. Multi-Website Support
This module is fully multi-website aware. It respects the `website_id` field on `website.page` records and uses Odoo's native website-switching logic to ensure that SEO metadata is correctly associated with the active website context. All controller logic uses `request.website` to filter relevant records.
