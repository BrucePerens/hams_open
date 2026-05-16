# 🔍 User Websites SEO Module (`user_websites_seo`)

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).*

**Context:** Technical documentation strictly for LLMs and Integrators.

## 1. 🏗️ Overview & Architecture
This module is a lightweight domain extension for `user_websites`. It connects our shared blog architecture with Odoo's native frontend SEO engine.

## 2. ⚙️ Technical Implementation Details
* **Model Injection:** It fuses the `website.seo.metadata` mixin into the `res.users` and `user.websites.group` models.
* **Authorization:** It appends the SEO metadata fields to the `SELF_WRITEABLE_FIELDS` property. Verified by `[@ANCHOR: res_users_self_writeable_fields]`.
* **Controller Interception:** Overrides the `/blog` route to inject the SEO-aware profile object into the QWeb context. Verified by `[@ANCHOR: controller_user_blog_index_seo_override]`.
* **Secure Elevation:** Escalate strictly for the write operation using the domain service account for users `[@ANCHOR: res_users_seo_write_elevation]` and groups `[@ANCHOR: user_websites_group_seo_write_elevation]`.
* **Soft Dependency Documentation:** The module uses a `post_init_hook` and `_register_hook` to dynamically install documentation `[@ANCHOR: soft_dependency_docs_installation]` if `knowledge.article` is present in the environment, without requiring a hard dependency on `manual_library` or `knowledge`.

---

<stories_and_journeys>
## 3. Architectural Stories & Journeys

For detailed narratives and end-to-end workflows, refer to the following:

### Stories
* [Group SEO Collaboration](user_websites_seo/docs/stories/group_seo_collaboration.md)
* [Individual SEO Control](user_websites_seo/docs/stories/individual_seo_control.md)
* [Seamless Documentation](user_websites_seo/docs/stories/seamless_documentation.md)

### Journeys
* [Administrator Configures Module](user_websites_seo/docs/journeys/administrator_configures_module.md)
* [User Optimizes Blog SEO](user_websites_seo/docs/journeys/user_optimizes_blog_seo.md)
</stories_and_journeys>
