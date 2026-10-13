# Story: Personal Site Management

As a **Community Member**, I want to create and manage my own personal website and blog so that I can share my thoughts and projects with the world.

## Scenarios

### Creating a Personal Home Page
- **Given** I am logged into the Odoo instance
- **When** I navigate to my personal URL (e.g., `/my-username/home`)
- **Then** I see a placeholder page inviting me to create my site.
- **When** I click the "Create My Site" button
- **Then** the system securely initializes my home page using a standard template ([@ANCHOR: UX_CREATE_SITE],
  routed by [@ANCHOR: user_websites:COMM_create_site]). Verified by `[@ANCHOR: test_tour_create_site]`.
- **And** I am redirected to my new home page where I can start editing.

### Writing a Blog Post
- **Given** I have an active site
- **When** I navigate to my blog index ([@ANCHOR: user_websites:COMM_user_blog_index])

- **Then** I can create my blog container, the first time, via [@ANCHOR: user_websites:COMM_create_blog]

- **And** I can create a new blog post ([@ANCHOR: UX_CREATE_BLOG_POST], routed by
  [@ANCHOR: COMM_create_blog_post]). Verified by `[@ANCHOR: test_tour_create_blog]`.

- **And** my followers will receive a weekly digest of my new content ([@ANCHOR: send_weekly_digest]). Verified by `[@ANCHOR: test_weekly_digest_mail_template]`.

### Editing and Publishing a Blog Post
- **Given** I just created a new blog post, or want to revise an existing one of my own (or one
  owned by a group website I belong to)
- **When** I open its dedicated edit form ([@ANCHOR: user_websites:UX_BLOG_POST_EDIT_FORM],
  routed by [@ANCHOR: user_websites:COMM_blog_post_edit])
- **Then** I see my post's current title and body, pre-filled, and a checkbox for whether it is
  published
- **When** I change the title and/or body and save ([@ANCHOR: user_websites:COMM_blog_post_edit_submit])
- **Then** my post is updated and I am redirected to its live page. Verified by
  `[@ANCHOR: test_owner_can_edit_own_blog_post]`.
- **And** if I am not the post's owner (or a member of the group that owns it), I am redirected
  away instead of seeing or changing its content
  ([@ANCHOR: user_websites:COMM_get_own_blog_post_for_edit]). Verified by
  `[@ANCHOR: test_blog_post_edit_denied_for_non_owner]` and
  `[@ANCHOR: test_blog_post_edit_denied_for_non_member]`.
- This exists because the generic website-builder "Edit" toolbar is gated on
  `website.group_website_designer`, a global, sitewide website-editing permission a personal-site
  owner does not and structurally cannot hold -- granting it would let any owner edit any website
  content sitewide, not just their own post. This dedicated form gives the owner a real way to
  title, write, and publish their own post without that sitewide permission change.

### Managing Content Quotas
- **Given** the administrator has set a global page limit
- **When** I attempt to create more pages than allowed
- **Then** the system enforces a quota check ([@ANCHOR: website_page_quota_check]) and prevents the creation of the new page with a helpful error message. Verified by `[@ANCHOR: test_page_quota_limit]`.

- **Given** the administrator has set a global blog limit
- **When** I attempt to create more blogs than allowed
- **Then** the system enforces a quota check ([@ANCHOR: user_websites_blog_quota_check]) and prevents the creation of the new blog with a helpful error message. Verified by `[@ANCHOR: test_blog_quota_limit]`.

- **Given** the administrator has set a global blog-post limit
- **When** I attempt to create more posts than allowed
- **Then** the system enforces a quota check ([@ANCHOR: user_websites_blog_post_quota_check]) and prevents the creation of the new post with a helpful error message. Verified by `[@ANCHOR: test_blog_post_quota_limit]`.

## Technical Notes
- Site creation uses a service account to bypass standard Odoo restrictions while maintaining strict ownership ([@ANCHOR: mixin_proxy_ownership_create]). Verified by `[@ANCHOR: test_mixin_ownership_validation]`.

- Page edits are restricted to the owner or administrators ([@ANCHOR: mixin_proxy_ownership_write]). Verified by `[@ANCHOR: test_mixin_ownership_validation]`.

- URL slugs are cached in Redis for high-performance routing ([@ANCHOR: slug_cache_invalidation]). Verified by `[@ANCHOR: test_slug_cache_invalidation]`.

## Creating Blog Posts

Creating a blog post enforces the caller's real create access (a public user can no longer create a post
owned by the shared public account), checks that the owner is plausible and that the blog post quota for
the user or group is not exceeded, and, for ordinary users, drops every field outside a fixed allow-list
(title, content, publish flag, owner, group, blog, website, meta and SEO fields). The record is then
created through the module's service account, and the cache-invalidation notice and Cloudflare purge are
sent for the new post's URLs.
`[@ANCHOR: user_websites:COMM_blog_post_create]`

## Home Address Is /<slug> (2026-10-12)

Bruce asked that every member and group site live at `hams.com/<callsign>`, with no `home`.
This section supersedes the `/my-username/home` addresses in the scenarios above.

- **Given** a member whose website name is `k6bp`, **when** anyone opens `/k6bp`, **then** the member's
  home page is served there ([@ANCHOR: user_websites:COMM_user_home_fallback]), or, for the owner of a
  site that has no home page yet, the placeholder with the "Create Your Home" button. Verified by
  `[@ANCHOR: test_member_site_home_url]`.
- **When** anyone opens the old `/k6bp/home`, **then** they get a permanent (301) redirect to `/k6bp`
  ([@ANCHOR: user_websites:COMM_user_home_legacy_redirect]), so bookmarks and search-engine entries
  keep working. The target is built from the stored slug, never from the request path.
- **When** a request for `/K6BP` arrives, **then** it is redirected (301) to the lower-case `/k6bp`.
- A single path segment that is not a member or group slug is handed back to Odoo's own page,
  redirect and attachment lookup, so `/contactus` and `website.rewrite` redirects are unaffected.
- Existing databases are migrated by `migrations/0.5/post-migration.py`
  ([@ANCHOR: user_websites:migrate_home_url_to_slug]), which renames each member or group page
  `/<slug>/home` to `/<slug>`.
- Reserved words (`home`, `admin`, `blog`, `my`, `privacy`, `manual`, `api`, and every other first
  segment of a platform route) cannot be claimed as a slug (`edge_routing.utils.RESERVED_SLUGS`).
  A guard test fails when a new static platform route is missing from that list.

## Owner Toolbar, Dashboard and Editor (2026-10-12)

Bruce, owner and administrator, on his own K6BP site: "I don't see any interface for creating anything."
The models and routes existed, but nothing on the site led to them, and the site wore hams.com's own menu and
footer. This section adds the owner's interface and the member site's own layout.

- **Given** I am signed in and I open any page of my own site (a member of the owning group, for a group site),
  **then** an owner toolbar sits above the site menu ([@ANCHOR: user_websites:UX_OWNER_TOOLBAR]): Manage site,
  New page, New blog post, Edit this page / post, Images, Privacy, Publish / Unpublish this page, and View as
  visitor. This holds for internal users, an administrator included, on their own slug only. Visitors and other
  members never see it. Verified by `[@ANCHOR: test_owner_toolbar_visible_to_owner_only]`.
- **When** I open Manage site, **then** the dashboard (`/my/site/<slug>`,
  [@ANCHOR: user_websites:UX_OWNER_DASHBOARD]) lists my pages, blog posts and images with Edit, Publish and
  Unpublish. Unpublish hides and keeps; nothing is deleted. Verified by
  `[@ANCHOR: test_owner_site_dashboard_lists_content]`.
- **When** I create or edit a page ([@ANCHOR: user_websites:UX_OWNER_PAGE_EDITOR]) or a post
  ([@ANCHOR: user_websites:UX_BLOG_POST_EDIT_FORM]), **then** I write in a WYSIWYG editor
  ([@ANCHOR: user_websites:UX_RICH_EDITOR]) with headings, bold, italic, underline, code, lists, quotes,
  alignment, links, images, undo and redo, and paste cleanup. I never type markup. Verified by
  `[@ANCHOR: test_tour_owner_site]` and `[@ANCHOR: test_owner_page_editor_renders_wysiwyg]`.
- **And** the server keeps only allow-listed markup whoever I am, including an internal administrator
  ([@ANCHOR: user_websites:member_html_sanitize]); hostile markup is stripped and reported through the
  content-violation pipeline. Verified by `[@ANCHOR: test_member_html_sanitize_strips_hostile_markup]`.
- **And** images are JPEG, PNG, GIF or WebP, at most 2 MB, 100 per site, stored as public attachments of my
  account ([@ANCHOR: user_websites:COMM_owner_image_upload]); writes are rate limited per user
  ([@ANCHOR: user_websites:owner_rate_limit]). Verified by `[@ANCHOR: test_owner_image_upload_rules]` and
  `[@ANCHOR: test_owner_write_rate_limited]`.
- **And** page addresses are `/<slug>/<address>` and cannot be a word the site itself uses
  ([@ANCHOR: user_websites:member_page_reserved_subpaths]). Verified by `[@ANCHOR: test_member_page_slug_reserved]`.
- **Then** a member site shows its own header (callsign, name, menu of its published pages and blog) and a small
  footer (callsign, a link to hams.com, Report, Privacy, Terms), and none of hams.com's menu, banners or footer
  ([@ANCHOR: user_websites:member_site_standalone_layout]). Verified by
  `[@ANCHOR: test_member_site_has_no_hams_chrome]`.
