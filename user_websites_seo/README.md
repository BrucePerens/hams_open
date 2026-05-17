# User Websites SEO (`user_websites_seo`)

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).*

This module is a lightweight domain extension for `user_websites`. It connects our shared blog architecture with Odoo's native frontend SEO engine.

## Technical Implementation
* **Model Injection:** It fuses the `website.seo.metadata` mixin into the `res.users` and `user.websites.group` models via `user.websites.seo.metadata.mixin`.
* **Authorization:** It appends the SEO metadata fields to the `SELF_WRITEABLE_FIELDS` property. This allows standard users to save their customized Meta Title and Description via the frontend widget. [@ANCHOR: res_users_self_writeable_fields]
* **Controller Interception:** It overrides the `/<slug>/blog` route. After the base controller prepares the data, this module injects the SEO-aware user or group record as the `main_object`, seamlessly activating the "Optimize SEO" UI menu for the blog owner while hiding it from guests. [@ANCHOR: controller_user_blog_index_seo_override]
* **Documentation:** Automatically installs its guide into the Knowledge/Manual Library via the `knowledge_docs` manifest entry. This uses the `zero_sudo` automatic bootstrap mechanism. [@ANCHOR: soft_dependency_docs_installation]

---

# Technical Documentation

<system_role>
**Context:** Technical documentation strictly for LLMs and Integrators.
</system_role>

## 1. 🏗️ Overview & Architecture
This module is a lightweight domain extension for `user_websites`. It connects our shared blog architecture with Odoo's native frontend SEO engine.

## 2. ⚙️ Technical Implementation Details
* **Model Injection:** It fuses the `website.seo.metadata` mixin into the `res.users` and `user.websites.group` models.
* **Authorization:** It appends the SEO metadata fields to the `SELF_WRITEABLE_FIELDS` property. Verified by `[@ANCHOR: test_self_writeable_fields]`.
* **Controller Interception:** Overrides the `/blog` route to inject the SEO-aware profile object into the QWeb context. Verified by `[@ANCHOR: test_controller_no_ssti_elevation]`.
* **Secure Elevation:** Escalate strictly for the write operation using the domain service account for users `[@ANCHOR: res_users_seo_write_elevation]` and groups `[@ANCHOR: user_websites_group_seo_write_elevation]`.
* **SSTI Protection:** The controller injects `main_object` into the QWeb context without elevating the recordset itself, ensuring that frontend templates cannot execute privileged operations.
* **Soft Dependency Documentation:** The module uses the `zero_sudo` automated installer to dynamically install documentation if `knowledge.article` or `manual.article` is present. Verified by `[@ANCHOR: test_soft_dependency_docs_installation]`.

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
