### Explanation of `ir.model.access.csv`

This CSV file is a critical part of Odoo's security system. It defines the base access control rights for various user groups on different data models. Each row in this file grants or denies permissions (Read, Write, Create, Delete) to a specific security group for a specific model.

Here's a breakdown of the rules in this file:

- **`content.violation.report`'s own `access_content_violation_report_*` rows** --
  moved to `content_moderation/security/ir.model.access.csv` on 2026-09-23,
  when that model was extracted out of this module into its own generic
  `content_moderation` module (see that module's own `ir.model.access.md`).
  `access_content_violation_report_admin` became
  `access_content_violation_report_moderator`, re-pointed at
  `content_moderation.group_content_moderation_moderator` (this module's
  own `group_user_websites_administrator` is wired into that group via
  `implied_ids`, in `security/user_websites_security.xml`'s own
  `noupdate="0"` block, so nothing changes for an existing Administrator).

- **`access_res_users_admin`**
  - **Group:** User Websites Administrator
  - **Model:** `res.users` (Users)
  - **Permissions:** Read, Write (but not Create or Delete).
  - **Purpose:** Allows administrators to modify user settings related to user websites (like page limits), but not to create or delete system users through this module's access rights.

- **`access_res_config_settings_admin`** -- **removed** (night_shift_todo.md
  "saving ANY Settings page can crash with an AccessError", full design
  writeup right after that entry). `ir.model.access.csv` grants are per
  (model, group), never per field, so this row was never actually scoped to
  the User Websites section of Settings -- it granted full read/write on
  the entire shared `res.config.settings` model, meaning User Websites
  Administrator (a content-moderation-tier role, not a System
  Administrator) could read and overwrite every OTHER installed module's
  settings too, including real credentials (confirmed concretely:
  `distributed_redis_cache`'s `redis_password`, `cloudflare`'s
  `cloudflare_api_token`). It was also the row that made the
  unconditional-every-save `res.groups.write()` in the old
  `res_config_settings.py` reachable at all. Managing this group's
  membership now goes through Odoo's own "Groups" screen
  (`base.action_res_groups`) or a user's own Access Rights tab, both
  already correctly gated to `base.group_system`-tier and neither wired
  through `res.config.settings.set_values()`.

- **`access_website_page_user`**
  - **Group:** User Website Owner
  - **Model:** `website.page`
  - **Permissions:** Full access.
  - **Purpose:** This is the baseline permission that allows a user in the "User Website Owner" group to create, view, edit, and delete their own website pages. Record rules will further restrict this to only their *own* pages.

- **`access_blog_post_user`**
  - **Group:** User Website Owner
  - **Model:** `blog.post`
  - **Permissions:** Full access.
  - **Purpose:** Allows users in the group to manage their own blog posts.

- **`access_blog_blog_user`**
  - **Group:** User Website Owner
  - **Model:** `blog.blog`
  - **Permissions:** Full access.
  - **Purpose:** Allows users to manage their own blogs.

- **`access_user_websites_group_admin`**
  - **Group:** User Websites Administrator
  - **Model:** `user.websites.group`
  - **Permissions:** Full access.
  - **Purpose:** Allows administrators to create, edit, and delete any group website configuration.

- **`access_user_websites_group_user`**
  - **Group:** Internal User
  - **Model:** `user.websites.group`
  - **Permissions:** Read-only.
  - **Purpose:** Allows any logged-in user to see the list of available group websites, but they cannot modify them.

- **`access_user_websites_group_public`**
  - **Group:** Public User
  - **Model:** `user.websites.group`
  - **Permissions:** Read-only.
  - **Purpose:** Allows non-logged-in users to see the list of public group websites.
