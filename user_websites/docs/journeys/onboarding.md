# Journey: First Time Site Setup

This journey describes the path a new user takes to establish their presence on the platform.

## Path: Onboarding and Creation

1. **Discovery**: The user navigates to the Community Directory ([@ANCHOR: user_websites:COMM_community_directory]) to see what others have built. Verified by `[ANCHOR: test_tour_community_directory]`.
2. **Accessing Profile**: The user clicks on their own name in the navbar or enters their slug directly (e.g., `/alice`).
3. **Routing**: The system resolves the slug and determines no site exists yet ([@ANCHOR: user_websites:COMM_user_home_fallback]). Verified by `[ANCHOR: test_tour_create_site]`.
4. **Placeholder**: The user sees the placeholder page with the "Create My Site" call-to-action.
5. **Initialization**: The user clicks the button, triggering the `create_site` controller ([ANCHOR: UX_CREATE_SITE]). Verified by `[ANCHOR: test_tour_create_site]`.

6. **Security Check**: The system verifies the user owns the slug and hasn't exceeded their page quota ([ANCHOR: website_page_quota_check]). Verified by `[ANCHOR: test_page_quota_limit]`.

7. **Provisioning**: The system creates the `website.page` record using a proxy service account ([ANCHOR: mixin_proxy_ownership_create]). Verified by `[ANCHOR: test_mixin_ownership_validation]`.
8. **Redirect**: The user is redirected to their live home page, now ready for customization.

## Path: Starting a Blog

1. **Blog Index**: The user navigates to `/alice/blog` [blog index route]. Verified by `[ANCHOR: test_tour_create_blog]`.

2. **First Post**: The user clicks "Create Blog Post" ([ANCHOR: UX_CREATE_BLOG_POST]). Verified by `[ANCHOR: test_tour_create_blog]`.
3. **Provisioning**: The system initializes the `blog.blog` container for the user if it doesn't exist and creates a "Welcome" post.
4. **Engagement**: Visitors can now subscribe to Alice's updates ([ANCHOR: UX_SUBSCRIBE]). Verified by `[ANCHOR: test_subscription_creation]`.

## Path: Identity Verification

1. **OTP Dispatch**: The system sends an email with an official OTP pass-code using the OTP mail template ([ANCHOR: ham_onboarding:otp_mail_template]).

2. **Official OTP Verification**: The system verifies the identity using an official OTP pass-code ([ANCHOR: ham_onboarding:action_verify_official_otp]).

## Addendum (2026-10-12): the site address is /<slug>

Steps 2 to 8 of "Onboarding and Creation" now use `/alice` rather than `/alice/home`: the placeholder, the "Create Your Home" button and the finished home page are all at `/alice`. `/alice/home` is a permanent redirect to `/alice` [legacy home redirect].

## Path: Writing Your First Page and Post (2026-10-12)

1. **Sign in and open your site**: `/alice`. The owner toolbar appears above the site menu [owner toolbar].
2. **Home page**: if it does not exist, "Create Your Home" makes it; "Edit this page" then opens the editor.
3. **New page**: toolbar, New page. Give a title, optionally an address, write in the editor, tick Published, Save [page save].
4. **New blog post**: toolbar, New blog post creates the blog container if needed and opens the post editor [new blog post].
5. **Manage**: toolbar, Manage site lists everything; Unpublish hides a page or post and keeps it [owner dashboard].
6. **See what visitors see**: toolbar, View as visitor.

## Anchor reference

### `user_blog_index(self, website_slug, tag=None, search=None, date_begin=None, date_end=None, page=1, **kwargs)` [@ANCHOR: user_websites:COMM_user_blog_index]

`GET /<uwslug:website_slug>/blog` and `/<slug>/blog/page/<n>` in `controllers/main.py` (`auth="public"`, `website=True`): the blog listing of a member or group site, the page a new member reaches in "Starting a Blog". `website_slug` selects the member or group; `tag`, `search`, `date_begin`, `date_end` and `page` are accepted for the stock blog filters but this handler does not apply them (the pager is built empty; unknown whether filtering is planned, to-do). Resolution: unknown slug follows a retired-slug 301 to `/blog` of the live site, else 404; a hidden member site (suspended or not eligible) or suspended group is 404. It lists up to 100 blogs and 100 posts of the site through the service account; visitors only see published posts, the owner (or any group member) also sees drafts. With no blog and no post it renders the placeholder page (`page_type` "blog") with the create buttons for the owner; otherwise `user_websites.blog_index`. Read only. Test: the tour `create_blog_tour.js` (`test_tour_create_blog`) opens `/blogtour/blog` as the owner and clicks the create button on the placeholder this handler renders.

### `user_home_legacy_redirect(self, website_slug, **kwargs)` [@ANCHOR: user_websites:COMM_user_home_legacy_redirect]

`GET /<uwslug:website_slug>/home` in `controllers/main.py` (`auth="public"`, `sitemap=False`): the old home address of a site. It looks the slug up as a member and as a group through the service account; if found it answers a permanent redirect (301, `local=True`) to `/<slug>`, where the slug is the one stored in the database (lower case, `[a-z0-9-]` by constraint) and URL-quoted, never the request path, so the target is always on this site and cannot loop, since a one-segment path never redirects to itself. If the slug is unknown it tries a retired-slug redirect, then Odoo's own page lookup (a stock page may own that path), then 404. No writes. Existing bookmarks and search-engine entries keep working. Test: `[@ANCHOR: test_member_site_home_url]`, the class `TestMemberSiteHomeUrl` in `tests/test_member_site_home_url.py`, which checks the 301 and its target, that hostile slugs like `//evil.example` never produce an off-site or protocol-relative Location, and the canonical-case and stock-page behaviour of `/<slug>`.

### Owner toolbar [@ANCHOR: user_websites:UX_OWNER_TOOLBAR]

The template `owner_toolbar` in `views/owner_site_templates.xml`, called from `user_navbar`: a dark bar "Your site" shown above the site's own navbar, on every page of the site, only to the person who owns it or a member of the owning group (help text in `data/documentation.html`). Buttons: Manage site (`/my/site/<slug>`), New page, New blog post (a CSRF-protected form posting to `/<slug>/create_blog_post`), Edit this page or Edit this post when the current page is one, Create your home page when on a home with no page, Images, Privacy, Publish or Unpublish this page (a form posting to the page's publish route with `next` back to the editor), Delete (not offered for the home) and View as visitor (adds `?visitor=1` to the current path). It reads `resolved_slug`, `main_object` and `page_type` from the render context and writes nothing itself. Test: `test_04_toolbar_visible_to_owner_only` in `tests/test_owner_site.py` (anchor `test_owner_toolbar_visible_to_owner_only`), and the tour `owner_site_tour.js`, whose first step checks the toolbar is on the owner's own site.

### `owner_page_save(self, site_slug, page_id="", title="", page_slug="", content="", is_published="", **kwargs)` [@ANCHOR: user_websites:COMM_owner_page_save]

`POST` in `controllers/owner_site.py` that creates or edits a member's page from the editor (signed in, CSRF checked, `@answers_site_response`). `site_slug` identifies the site (the caller must own it or belong to the group, else 404; suspended and ineligible owners get the 403 explanation pages). `page_id` empty means a new page, otherwise the id of an owned page (digits only). `title` is required, at most `MAX_TITLE_CHARS`. `page_slug` is the new page's address part (defaults to a slug of the title; must match `PAGE_SLUG_RE` and not be reserved; refused when `/<slug>/<page_slug>` already exists). `content` is the body, run through `sanitize_member_html`, at most `MAX_BODY_CHARS`; hostile markup is stripped and also filed as an automated violation report. `is_published` is a tick box. Writes are rate limited per user (`WRITES_PER_MINUTE`). On a refusal (`title`, `too_long`, `address`, `exists`) the editor is shown again with status 400 and what the member typed; a page whose arch is not editable by this editor redirects back with `error=not_editable`. A new page is created by the service account, an existing one is written in the member's own environment so the ownership guard and quotas still apply. Returns a redirect to the editor with `saved=1`. Test: `test_owner_page_create_edit_publish` and the other page-save tests in `tests/test_owner_site.py`.

### `create_blog_post(self, website_slug, **kwargs)` [@ANCHOR: user_websites:COMM_create_blog_post]

`POST /<uwslug:website_slug>/create_blog_post` in `controllers/main.py` (signed in, CSRF checked), behind the toolbar's New blog post button. It applies the same ownership rules as site creation (own site or group member; 404 for suspended; 303 to the owner tools if not eligible), finds the owner's or group's blog container or creates one, then creates a blank post named "New Post" in it with the author set explicitly (the member's partner; for a group the requesting member's partner) and redirects to the post's editor. Created as the service account because the member has no create access on blog posts. Test: `[@ANCHOR: test_tour_owner_site]`, the browser tour `owner_site_tour.js` run by `tests/test_ui_tours.py`, which signs in as an owner, checks the owner toolbar is on the site, starts a new blog post from the toolbar, writes and formats the body in the rich editor (bold, undo, redo), publishes and saves, and checks the post is live.

### Owner dashboard [@ANCHOR: user_websites:UX_OWNER_DASHBOARD]

The page `/my/site/<slug>` rendered from the template `owner_site_dashboard` in `views/owner_site_templates.xml` by `owner_site_dashboard(self, site_slug, **kwargs)` in `controllers/owner_site.py` (help text in `data/documentation.html`). It lists the owner's pages with address, status and actions, blog posts, images and the Deleted list, with the notices "Saved", "Restored", "Deleted" and the errors for home deletion, blocked home and image upload; a form creates the home page when none exists. Unpublish hides a page and keeps it as a draft; Delete removes it from the site but it can be restored. It changes nothing on display; each button posts to its own CSRF-protected route. Only the owner or a group member sees it. Test: `test_08_dashboard_lists_content` in `tests/test_owner_site.py` (anchor `test_owner_site_dashboard_lists_content`) checks the dashboard lists the owner's content.
