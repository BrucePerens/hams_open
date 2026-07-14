# Story: Group Websites

As a **Group Member**, I want to manage a shared website and blog for my team or club so that we can communicate our collective mission and updates.

## Scenarios

### Creating a Group Site
- **Given** I am a member of an Odoo group that has a linked `user.websites.group` record.
- **When** I navigate to the group's URL (e.g., `/my-team/home`).
- **Then** I see a placeholder page if the site hasn't been created yet.
- **When** I click "Create Site", the system initializes the home page for the group ([@ANCHOR: UX_CREATE_SITE]). Verified by `[@ANCHOR: test_group_site_creation]`.
- **And** the group is assigned as the owner of the page.

### Shared Blogging
- **Given** the group has an active site.
- **When** any member of the group navigates to `/my-team/blog`.
- **Then** they can create new blog posts ([@ANCHOR: UX_CREATE_BLOG_POST]). Verified by `[@ANCHOR: test_group_blog_post_creation]`.
- **And** all posts are attributed to the group's shared blog container.

## Technical Notes
- Group site routing follows similar logic to personal sites ([@ANCHOR: controller_user_websites_home]). Verified by `[@ANCHOR: test_group_site_routing]`.

- Access to edit or delete group pages is restricted to members of the associated Odoo group ([@ANCHOR: mixin_proxy_ownership_write]). Verified by `[@ANCHOR: test_mixin_ownership_validation]`.

- Slugs for groups are also cached in Redis ([@ANCHOR: group_slug_cache_invalidation]). Verified by `[@ANCHOR: test_group_slug_cache_invalidation]`.

- Group members can be unsubscribed from notifications using a secure link ([@ANCHOR: controller_unsubscribe_digest]). Verified by `[@ANCHOR: controller_unsubscribe_digest]`.
