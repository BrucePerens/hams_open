# User Websites SEO

This module allows users to optimize their personal and group blogs for search engines (like Google) and social media directly from the website or backend.

## Overview
Search Engine Optimization (SEO) is critical for making your content discoverable. This module integrates Odoo's powerful SEO tools into the `user_websites` framework, allowing blog owners and group members to manage their online presence securely.

## Key Features
- **Frontend SEO Widget:** Edit meta titles, descriptions, and keywords directly while viewing your blog, individual pages, or specific blog posts.
- **Social Media Previews:** Customize the images and titles that appear when your content is shared on platforms like Facebook or X (Twitter).
- **SEO Keywords:** Add specific keywords to help search engines understand the topics of your site.
- **Secure Editing:** Our "Zero-Sudo" architecture ensures you can only edit SEO data for content you own or groups you belong to.
- **Backend Management:** SEO fields are conveniently located in a dedicated tab on user profiles, groups, website pages, and blog posts.

## How to Use

### From the Website
1. Log in and navigate to the page or blog post you want to optimize (e.g., `/your-slug/home` or `/your-slug/blog/post/1`).
2. Click the **Site** menu in the top bar.
3. Select **Optimize SEO**.
4. Update your **Title**, **Description**, and **Keywords** in the dialog.
5. Click **Save**.

### From the Backend
1. Open the record you wish to edit (User Profile, Group, Website Page, or Blog Post).
2. Click on the **SEO Metadata** tab (or the equivalent section for pages/posts).
3. Modify the fields as needed (Title, Description, Keywords, Social Media Image).
4. Click **Save**.

---

# Technical Documentation

<system_role>
**Context:** Technical documentation for Integrators and Developers.
</system_role>

## 1. Architecture
This module extends `user_websites` by adding SEO metadata to users and groups through inheritance of `website.seo.metadata`.

## 2. Security & Zero-Sudo
We strictly follow the Zero-Sudo mandate. Privileged writes are handled via a dedicated service account: `user_websites.user_websites_service_account`.

*   **Model Mixin:** `user.websites.seo.metadata.mixin` centralizes the secure write logic.
    *   **Developer Usage:** To use this mixin on a model, inherit from `user.websites.seo.metadata.mixin` and you **must** implement the `_check_seo_write_permission(self)` method to define who is authorized to edit SEO fields.
    *   **Bypassing Logic:** Developers can pass `{"skip_seo_metadata_mixin": True}` in the context to bypass the mixin's specialized write behavior.
*   **Self-Writable Fields:** SEO fields are whitelisted in `res.users` to allow users to edit their own profiles without elevated backend rights. `[@ANCHOR: COMM_res_users_self_writeable_fields]`

## 3. Implementation Details
*   **Controller Override:** `UserWebsitesSEOController` intercepts the blog index route to inject the `main_object`. This is required for the Odoo frontend SEO widget to function. `[@ANCHOR: COMM_controller_user_blog_index_seo_override]`
*   **Traceability:** All critical logic is mapped to semantic anchors and verified by the test suite.

## 4. 🔗 Semantic Anchors & Traceability

| Anchor | Description | Verified By |
|--------|-------------|-------------|
| `[@ANCHOR: COMM_res_users_self_writeable_fields]` | Whitelisting SEO fields for users. | `COMM_test_self_writeable_fields` |

| `[@ANCHOR: COMM_res_users_seo_write_elevation]` | Elevated write for user SEO metadata. | `COMM_test_check_access_rule_res_users` |

| `[@ANCHOR: COMM_user_websites_group_seo_write_elevation]` | Elevated write for group SEO metadata. | `COMM_test_check_access_rule_user_websites_group` |

| `[@ANCHOR: COMM_controller_user_blog_index_seo_override]` | Controller override for SEO widget activation. | `COMM_test_controller_no_ssti_elevation` |

| `[@ANCHOR: COMM_soft_dependency_docs_installation]` | Automatic documentation installation signaling. | `COMM_test_soft_dependency_docs_installation` |

| `[@ANCHOR: COMM_test_seo_widget_tour]` | UI tour for SEO optimization. | `COMM_test_seo_widget_tour` |

| `[@ANCHOR: COMM_test_xpath_rendering_res_users]` | Backend view rendering for users. | `COMM_test_xpath_rendering_res_users` |

| `[@ANCHOR: COMM_test_xpath_rendering_user_websites_group]` | Backend view rendering for groups. | `COMM_test_xpath_rendering_user_websites_group` |

## 5. Multi-Website & Multi-Tenant Support
- **Multi-Tenancy:** Inherits from `res.users` and `user.websites.group`, ensuring natural isolation between organizations.
- **Multi-Website:** Fully compatible with Odoo's multi-website routing and context switching.

## External Dependencies
*None.*
