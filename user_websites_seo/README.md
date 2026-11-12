# User Websites SEO

This module allows users to optimize their personal and group blogs for search engines (like Google) and social media directly from the website or backend.

## Overview
Search Engine Optimization (SEO) is critical for making your content discoverable. This module integrates Odoo's powerful SEO tools into the `user_websites` framework, allowing blog owners and group members to manage their online presence securely.

## Key Features
- **Frontend SEO Widget:** Edit meta titles, descriptions, and keywords directly while viewing your blog, individual pages, or specific blog posts.
- **Social Media Previews:** Customize the images and titles that appear when your content is shared on platforms like Facebook or X (Twitter).
- **SEO Keywords:** Add specific keywords to help search engines understand the topics of your site.
- **Secure Editing:** Our "Zero-Sudo" architecture ensures you can only edit SEO data for content you own or groups you belong to.
- **Backend Management:** SEO fields are conveniently located in a dedicated tab on user profiles, groups, website pages, blogs, and blog posts.

## How to Use

### From the Website
1. Log in and navigate to the page or blog post you want to optimize (e.g., `/your-slug/home`, `/your-slug/blog`, or a post at `/blog/<blog-id>/post/<post-id>`).
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
This module extends `user_websites` by adding the SEO metadata fields to groups through inheritance of `website.seo.metadata` (`models/user_websites_group.py`), and by putting the secure-write mixin described below on `res.users`, `user.websites.group`, `website.page`, `blog.blog`, and `blog.post`. The SEO fields it manages are `website_meta_title`, `website_meta_description`, `website_meta_keywords`, `website_meta_og_img` (the social media preview image), and `seo_name`.

## 2. Security & Zero-Sudo
We strictly follow the Zero-Sudo mandate: code never calls Odoo's `.sudo()` (which bypasses all access rules); when a step needs more rights than the user has, it runs just that step as a narrowly scoped service account via `with_user(svc_uid)`. Privileged writes are handled via a dedicated service account: `user_websites.user_websites_service_account`.

*   **Model Mixin:** `user.websites.seo.metadata.mixin` centralizes the secure write logic. When a `write()` includes SEO fields, the mixin first calls the model's `_check_seo_write_permission()` (which raises `AccessError` to refuse the whole write), then writes the non-SEO fields as the caller under normal Odoo access rules, then writes the SEO fields as the service account. The superuser and members of `user_websites.group_user_websites_administrator` skip these checks.
    *   **Developer Usage:** To use this mixin on a model, inherit from `user.websites.seo.metadata.mixin` and you **must** implement the `_check_seo_write_permission(self)` method to define who is authorized to edit SEO fields (the mixin's default raises `NotImplementedError`). The built-in rules: a user may edit only their own `res.users` record, only members may edit a group's, and `website.page`, `blog.blog`, and `blog.post` require normal write access to the record (which `user_websites` limits to its owner or group members).
    *   **Bypassing Logic:** Developers can pass `{"skip_seo_metadata_mixin": True}` in the context to bypass the mixin's specialized write behavior.
*   **Self-Writable Fields:** SEO fields are whitelisted in `res.users` to allow users to edit their own profiles without elevated backend rights (section 4, `COMM_res_users_self_writeable_fields`).

## 3. Implementation Details
*   **Controller Override:** `UserWebsitesSEOController` intercepts the blog index route to inject the `main_object`. This is required for the Odoo frontend SEO widget to function. The base `user_websites` controller already sets `main_object` to the profile user or group, but loads it as the service account; the override pre-reads the SEO fields, then re-binds the user or group record to the visiting user's own access rights before rendering, so that record is not exposed to the template (and to server-side template injection, SSTI) with service-account privileges (section 4, `COMM_controller_user_blog_index_seo_override`).
*   **Traceability:** All critical logic is mapped to semantic anchors and verified by the test suite.

## 4. 🔗 Semantic Anchors & Traceability

Each anchor below is explained in full. The test that verifies it is named in its entry.

### `SELF_WRITEABLE_FIELDS` on `res.users` [@ANCHOR: COMM_res_users_self_writeable_fields]

A property of the `res.users` extension in `models/res_users.py` (`ResUsersSEO`, which also inherits
`user.websites.seo.metadata.mixin`). It returns `super().SELF_WRITEABLE_FIELDS + list(self._get_seo_fields())`: the stock list
of fields a user may write on their own record, plus the five SEO fields `website_meta_title`, `website_meta_description`,
`website_meta_keywords`, `website_meta_og_img` and `seo_name`. Without it a portal or other non-admin user would get an access
error when saving their own SEO data, because Odoo restricts writes to `res.users` to this list. It takes no arguments, has no
side effects and is evaluated per access. Adding a field to `_get_seo_fields` extends it automatically. Whether the user
may write the field is then decided by the mixin's `write` and `_check_seo_write_permission` (own record only). Test:
`test_self_writeable_fields` in `tests/test_seo_models.py`.

### `ResUsersSEO._check_seo_write_permission(self)` [@ANCHOR: COMM_res_users_seo_write_elevation]

The authorization half of the SEO write on users, in `models/res_users.py`. The mixin's `write` calls it when a write contains
SEO fields. It returns nothing when every record in `self` is the current user's own record (`record.id == self.env.user.id`
for all), and otherwise raises `AccessError("You can only modify your own SEO metadata.")`, which refuses the whole write
before anything is saved. The "elevation" in the name is the step after it passes: the mixin then performs the SEO-field write
as `user_websites.user_websites_service_account` (a service account whose real companies are the main company only), so a user
whose own company is not the main one can still hit an access error there, which a test pins down. Administrators in
`user_websites.group_user_websites_administrator` and the superuser never call this check. No database writes or locks of its
own. Tests: `test_check_access_rule_res_users` and
`test_check_access_rule_res_users_seo_self_write_survives_a_non_main_company` in `tests/test_seo_models.py`; the UI tour
also runs it.

### `UserWebsitesGroupSEO._check_seo_write_permission(self)` [@ANCHOR: COMM_user_websites_group_seo_write_elevation]

The same check for `user.websites.group` (`models/user_websites_group.py`; the model also inherits `website.seo.metadata`, which
supplies the SEO fields). It passes when the current user is in `member_ids` of every group in `self`, otherwise raises
`AccessError("You can only modify SEO metadata for groups you are a member of.")`, before any value is written. After it
passes, the mixin writes the SEO fields as the user_websites service account. Because that account's companies are only the
main company, a member of a group that belongs to another company gets an `AccessError` on the elevated write; the test
`test_check_access_rule_user_websites_group_seo_self_write_denied_for_a_non_main_company` records this as current behaviour
(to-do: decide whether that is wanted). No side effects. Tests: `test_check_access_rule_user_websites_group` and the
non-main-company test in `tests/test_seo_models.py`.

### `UserWebsitesSEOController.user_blog_index(...)` [@ANCHOR: COMM_controller_user_blog_index_seo_override]

Route `["/<uwslug:website_slug>/blog", "/<uwslug:website_slug>/blog/"]` (`type="http"`, `auth="public"`, `website=True`) in
`controllers/main.py`, overriding the base `user_websites` controller of the same name.

| Argument | Type | Meaning |
|---|---|---|
| `website_slug` | `str` | Slug of the member or group whose blog is shown (from the URL converter). |
| `tag`, `search`, `date_begin`, `date_end` | `str` or `None` | Blog filters, passed to the parent unchanged. |
| `page` | `int` | Page number, default 1. |
| `**kwargs` | dict | Other query arguments, passed on. |

It calls the parent and, if the result is a `Response` with a `qcontext`, takes `profile_user` or `profile_group` from it. The
base controller loaded that record as the service account. The override first reads the SEO fields (`record.read(
list(record._get_seo_fields()))`) so they are in the ORM cache, then rebinds the record to the visiting user with
`with_user(request.env.user)` and sets it as `main_object` (and `profile_user` or `profile_group`). `main_object` is what
Odoo's SEO widget and meta tags need; rebinding means the template can no longer reach other data with service-account
rights, which closes a server-side-template-injection (SSTI) route. A response that is not a `Response` or has no `qcontext`
(a redirect, a 404) is returned unchanged. Returns the parent's response. Public access; no writes. Tests:
`test_controller_no_ssti_elevation` in `tests/test_seo_controller.py`, and the UI tour.

### `post_init_hook(env)` [@ANCHOR: COMM_soft_dependency_docs_installation]

Install hook in `hooks.py`. `env` is the installing environment. It reads the system parameter
`user_websites_seo.docs_installed` through `zero_sudo.security.utils`; when it is not set, it makes sure the service account
`user_websites.user_websites_service_account` exists (`_get_service_uid`, which raises if missing) and then sets the parameter
to `"True"`. The effect is a flag that tells the rest of the system that the SEO documentation is in place. It is idempotent:
the second call finds the flag and does nothing. The whole body is inside `try`; any exception is logged with its traceback
(`audit-ignore-catch-all`) and the install continues, since the flag is a signal and not a requirement. Returns nothing. Unknown:
no test currently carries the verifying tag `COMM_test_soft_dependency_docs_installation`; to-do: add one that runs the hook
twice and checks the parameter.

### SEO widget tour [@ANCHOR: COMM_test_seo_widget_tour]

A test marker, not a function. `tests/test_seo_ui_tour.py::test_01_seo_widget_tour` runs the browser tour `user_websites_seo_tour`
(`static/src/js/tours/user_websites_seo_tour.js`). The tour opens the Users list (removing the default Internal Users filter),
searches for the test user "SEO UI Test User", opens the record, clicks the **SEO Metadata** tab, types a title, a description
and keywords, saves through the standard save helper, and returns to the list; the tour fails if the tab, a field or the save control
is missing or the save is refused. The test itself asserts nothing else (it does not read the stored values back; to-do).
This exercises the view, the permission check and the elevated write in one pass; the permission anchors cite it as a verifier.

### Users form view [@ANCHOR: COMM_test_xpath_rendering_res_users]

A test marker for the view `view_users_form_user_websites_seo` in `views/res_users_views.xml`, which inherits
`user_websites.view_users_form_user_websites` and adds a notebook page "SEO Metadata" (`name="user_websites_seo_settings"`)
after the user_websites settings page. The page has two groups: SEO Data (`seo_name`, `website_meta_title`,
`website_meta_description`, `website_meta_keywords`) and Social Media (`website_meta_og_img`). The test
`test_xpath_rendering_res_users` in `tests/test_seo_models.py` fetches the `base.view_users_form` architecture and asserts the page
name and the `website_meta_og_img` field are present, which fails if the xpath stops matching after an upstream change.

### Group form view [@ANCHOR: COMM_test_xpath_rendering_user_websites_group]

A test marker for the view `view_user_websites_group_form_seo` in `views/user_websites_group_views.xml`, which inherits
`user_websites.view_user_websites_group_form` and adds a page "SEO Metadata" (`name="group_seo_settings"`) inside the
notebook, with the same two groups of fields as the user page. The test `test_xpath_rendering_user_websites_group` fetches that
form's architecture and asserts that `group_seo_settings` and `website_meta_og_img` are in it. The page can be edited only by
members of the group, through the check above.

## 5. Multi-Website & Multi-Tenant Support
- **Multi-Tenancy:** Inherits from `res.users` and `user.websites.group`, ensuring natural isolation between organizations: the SEO fields are stored on those existing records, so they are covered by the same record rules (including `user_websites`' multi-company rule on `user.websites.group`) as the records themselves.
- **Multi-Website:** Fully compatible with Odoo's multi-website routing and context switching.

## External Dependencies
*None.*
