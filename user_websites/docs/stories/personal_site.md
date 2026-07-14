# Story: Personal Site Management

As a **Community Member**, I want to create and manage my own personal website and blog so that I can share my thoughts and projects with the world.

## Scenarios

### Creating a Personal Home Page
- **Given** I am logged into the Odoo instance
- **When** I navigate to my personal URL (e.g., `/my-username/home`)
- **Then** I see a placeholder page inviting me to create my site.
- **When** I click the "Create My Site" button
- **Then** the system securely initializes my home page using a standard template ([@ANCHOR: UX_CREATE_SITE]). Verified by `[@ANCHOR: test_tour_create_site]`.
- **And** I am redirected to my new home page where I can start editing.

### Writing a Blog Post
- **Given** I have an active site
- **When** I navigate to my blog index ([@ANCHOR: controller_user_blog_index])

- **Then** I can create a new blog post ([@ANCHOR: UX_CREATE_BLOG_POST]). Verified by `[@ANCHOR: test_tour_create_blog]`.

- **And** my followers will receive a weekly digest of my new content ([@ANCHOR: send_weekly_digest]). Verified by `[@ANCHOR: test_weekly_digest_mail_template]`.

### Managing Content Quotas
- **Given** the administrator has set a global page limit
- **When** I attempt to create more pages than allowed
- **Then** the system enforces a quota check ([@ANCHOR: website_page_quota_check]) and prevents the creation of the new page with a helpful error message. Verified by `[@ANCHOR: test_page_quota_limit]`.

## Technical Notes
- Site creation uses a service account to bypass standard Odoo restrictions while maintaining strict ownership ([@ANCHOR: mixin_proxy_ownership_create]). Verified by `[@ANCHOR: test_mixin_ownership_validation]`.

- Page edits are restricted to the owner or administrators ([@ANCHOR: mixin_proxy_ownership_write]). Verified by `[@ANCHOR: test_mixin_ownership_validation]`.

- URL slugs are cached in Redis for high-performance routing ([@ANCHOR: slug_cache_invalidation]). Verified by `[@ANCHOR: test_slug_cache_invalidation]`.
