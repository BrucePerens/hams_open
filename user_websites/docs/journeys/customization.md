# Journey: Extension and Customization

This journey describes how the platform can be extended or customized by other modules or administrators using standardized "dropzones". A dropzone is an empty element with a fixed `id` that this module keeps in its templates and views; another module's view inheritance targets it with an XPath, so the extension keeps working when the surrounding markup changes. Each dropzone and XPath anchor is explained in full in the "Anchor reference" at the end of this file (ADR 0113); the entry names are given in square brackets.

## Path: UI Customization via Dropzones

The module provides several stable XPath anchors (dropzones) in its QWeb templates to allow safe extension of the user interface without breaking core functionality.

1. **Navigation**: Extensions can inject items into the global navbar [navbar dropzone].
2. **Layout**: Custom CSS or JS can be injected into the main website layout [layout dropzone].
3. **Snippets**: New drag-and-drop building blocks can be added to the snippets sidebar [snippets dropzone].
4. **User Settings**: Additional configuration options can be added to the user settings portal [users dropzone].

## Path: Architectural Extension

1. **Rendering**: Custom logic can be hooked into the rendering process of various components like the blog post [blog post xpath] or the navbar [navbar dropzone].
2. **Templates**: New portal templates can be registered via the templates dropzone [templates dropzone].

## Technical Notes
- These dropzones are verified by automated rendering tests to ensure they remain stable [layout dropzone, snippets dropzone].
- The system uses a specific XPath rendering pattern for high-performance UI assembly [layout dropzone].

## Anchor reference

### Navbar dropzone and navbar injection [@ANCHOR: dropzone_navbar]

In the template `user_websites_templates.xml`, an XPath (`//header`, position `after`, anchored as `xpath_rendering_navbar` [@ANCHOR: xpath_rendering_navbar]) adds, right after the website header, an empty `<div id="user_websites_dropzone_navbar">` and then, when the page belongs to a member or group site (`resolved_slug` and `member_site` set), a call of the template `user_websites.user_navbar`. The div is the dropzone: a module that inherits `website.layout` can put an XPath on `//div[@id='user_websites_dropzone_navbar']` with position `inside` to add links or banners above the member's own navbar without depending on the markup around it. Nothing is rendered inside it by this module. Takes no arguments; its context is the QWeb render context (`resolved_slug`, `profile_user`, `profile_group`, `is_owner`). Test: `tests/test_xpath_rendering.py::test_07_navbar_rendering` creates a member with a published home and fetches it, checking the dropzone-related markup, the `user_websites_slug` meta tag and the element `userNavbarNav`.

### Layout dropzone [@ANCHOR: dropzone_layout]

In `views/website_layout.xml`, the template `layout_inherit_report_violation` inherits `website.layout` with an XPath on `//footer` (position `after`, anchored as `xpath_rendering_layout` [@ANCHOR: xpath_rendering_layout]) that adds `<div id="user_websites_dropzone_layout">`, followed by a call of `report_violation_modal`. The empty div is the place for a module to inject site-wide extra CSS or JavaScript tags or markup after the footer, targeted by XPath on its `id`. The same template has a second XPath before the footer that shows the report widget only for personal-website pages. Rendering cost is one extra empty element. Tests: `[@ANCHOR: test_dropzone_layout]`, `tests/test_xpath_rendering.py::test_06_layout_templates`, which fetches the home page as a guest and checks the report modal (`reportViolationModal`) and the guest email field are in the output; the same test method covers the XPath pattern.

### Snippets dropzone [@ANCHOR: dropzone_snippets]

In `views/snippets.xml`, the template `user_websites_snippet_category` inherits `website.snippets` with an XPath on `/*` (position `inside`) that adds `<div id="user_websites_dropzone_snippets">` and a panel `snippet_user_websites` titled "Profile Widgets", whose body `user_websites_snippets_body` is empty for other modules to fill. Another module adds drag-and-drop blocks to the website builder sidebar by inheriting `website.snippets` with an XPath on either the dropzone div or the panel body. This module registers no snippets itself. Test: `[@ANCHOR: test_dropzone_snippets]`, `tests/test_xpath_rendering.py::test_04_snippets`, which pulls the combined architecture of `website.snippets` and checks it contains `id="snippet_user_websites"`.

### Users dropzone [@ANCHOR: dropzone_users]

In `views/res_users_views.xml`, the view `view_users_form_user_websites` inherits `base.view_users_form` and adds a notebook page "User Websites" (name `user_websites_settings`) containing `<div id="user_websites_dropzone_users">` above the Configuration group (`website_slug`, `website_page_limit`, `privacy_show_in_directory`) and, for user-websites administrators, a Moderation and Trust group (`violation_strike_count`, `is_suspended_from_websites`, a pardon button). A module extends the user settings by inheriting `base.view_users_form` with an XPath on that div (position `inside` or `after`) to add its own fields on the same page. The page is visible to those who may open the user form. Test: `tests/test_xpath_rendering.py::test_02_res_users`, which compiles the form and checks the page name is in the arch.

### Blog post form XPath [@ANCHOR: xpath_rendering_blog_post]

In `views/blog_post_views.xml`, the view `view_blog_post_form_inherit_user_websites` inherits `website_blog.view_blog_post_form`. Its XPath on `//field[@name='blog_id']` (position `after`) adds the ownership fields `owner_user_id` and `user_websites_group_id` (group creation disabled) and then an empty div `user_websites_dropzone_blog_post` (anchor `dropzone_blog_post`) where other modules can add their own back-office fields for a blog post. This is the extension point for the owner and group of a post; the ownership checks themselves are in the owned mixin, not in this view. Test: `tests/test_xpath_rendering.py::test_03_blog_post`, which compiles the blog post form view and checks that `user_websites_group_id` is in the arch.

### Templates dropzone [@ANCHOR: dropzone_templates]

In `views/user_websites_templates.xml`, the template `portal_my_home_appeal` inherits `portal.portal_my_home` with an XPath on the `o_portal_my_home` div (position `before`, anchor `xpath_rendering_appeal`) that adds `<div id="user_websites_dropzone_templates">` above the portal's own cards, followed by the suspension and appeal alert and the Privacy and Data links. Other modules use the empty div to register extra portal templates or cards on `/my/home` by inheriting `portal.portal_my_home` and targeting the div's `id`. It is rendered for every signed-in portal user. Test: `tests/test_xpath_rendering.py::test_05_portal_templates`, which logs in as a portal user, loads `/my/home` and checks the Privacy and Data text and the dropzone id are in the response.
