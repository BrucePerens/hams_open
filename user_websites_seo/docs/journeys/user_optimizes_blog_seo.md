# User Optimizes Blog SEO Journey

This journey describes the technical flow when a user optimizes the SEO metadata for their blog index page.

1. **Route Access**: The user visits their blog at `/<slug>/blog`.
2. **Controller Interception**: The `user_blog_index` route in `UserWebsitesSEOController` is triggered.
   - *Code Reference*: `[ANCHOR: controller_user_blog_index_seo_override]`
3. **Context Injection**: The controller identifies the `profile_user` or `profile_group` in the rendering context and sets it as the `main_object`. This enables the Odoo frontend SEO widget for the current page.
4. **User Input**: The user opens the "Optimize SEO" widget and enters new metadata.
5. **Backend Update**: Clicking "Save" sends a `write` request to the server.
6. **Authorization & Elevation**:
   - The module appends SEO fields to writeable fields: `[ANCHOR: res_users_self_writeable_fields]`
   - For a personal blog: `ResUsersSEO.write()` is called. It checks if the user is modifying their own record. If so, it elevates the write using the `user_websites` service account.
     - *Code Reference*: `[ANCHOR: res_users_seo_write_elevation]`
   - For a group blog: `UserWebsitesGroupSEO.write()` is called. It checks for group membership and elevates the write similarly.
     - *Code Reference*: `[ANCHOR: user_websites_group_seo_write_elevation]`
7. **Completion**: The metadata is updated in the database, and the frontend reflects the changes.
8. **Automated Verification**: This flow is verified by the SEO UI tour.
   - *Test Reference*: `[ANCHOR: test_seo_widget_tour]`
