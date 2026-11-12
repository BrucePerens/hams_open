# Story: Personal Site Management

As a **Community Member**, I want to create and manage my own personal website and blog so that I can share my thoughts and projects with the world.

## Scenarios

Each anchor below has its own entry, read from the code it points at. Source paths are relative to the
`user_websites` module directory unless they begin with another module name. Tests cite an anchor in their own
code; the test ids named here are the ones this story already cites.

### Creating a Personal Home Page
- **Given** I am logged into the Odoo instance
- **When** I navigate to my personal URL (e.g., `/my-username/home`)
- **Then** I see a placeholder page inviting me to create my site.
- **When** I click the "Create My Site" button
- **Then** the system securely initializes my home page using a standard template.
- **And** I am redirected to my new home page where I can start editing.

### Create Site Route: `COMM_create_site` [@ANCHOR: user_websites:COMM_create_site]

`POST /<website_slug>/create_site` is `UserWebsitesController.create_site(self, website_slug, **kwargs)` in
`controllers/main.py`: `auth="user"`, `methods=["POST"]`, `csrf=True`, `website=True`. The only argument is the site
slug taken from the path; extra form fields are ignored. It resolves the slug to a member or a group through
the service account, then returns 404 when the slug is unknown, when a member site is not the caller's own, when the site
is suspended, or when the caller is not a member of the group. A member who fails the eligibility hook is redirected (303)
to `/my/site/<slug>` where the owner tools explain what is missing. It then asks `_owner_home()` for the state of
`/<slug>`: `blocked` redirects to the owner tools with `error=home_blocked`; `deleted` and `unpublished` pages are
restored or published in place (`no_cow` write); only `none` creates a new `website.page` of type `qweb` named
"<name> Home" whose arch calls `user_websites.template_default_home`. The member's name is XML-escaped before it enters
the arch. All writes run as the service account. Returns a redirect (302 or 303) to the home page. Not idempotent for
`none` only until the first success; after that the page exists and the route just redirects. Verified by
`[@ANCHOR: test_tour_create_site]` (`tests/test_ui_tours.py`, `static/tests/tours/create_site_tour.js`).

### Create-Site Button: `UX_CREATE_SITE` [@ANCHOR: UX_CREATE_SITE]

The "Create Your Home" button is a form in the `placeholder_page` template (`views/user_websites_templates.xml`) with the
CSS class `o_tour_create_site_btn`. It is shown only to the owner of a site that has no served home page: for a visitor
the same page says the user has not created their home yet. It posts, with a CSRF token, to `/<slug>/create_site` (home) or
`/<slug>/create_blog` (blog), and the label changes with the state that `_owner_home()` found: "Create Your Home", "Publish
Your Home" or "Restore Your Home". A small help link goes to the documentation page with `#UX_CREATE_SITE` in the address.
The tour `create_site_tour.js` clicks the button and waits for the new home page to render.

### A Home Page That Already Exists
- **Given** my home page already exists at `/<callsign>` but is unpublished, or I deleted it
- **When** I open my address
- **Then** the page offers "Publish Your Home" or "Restore Your Home" instead of "Create Your Home",
  because the page and the Create button ask the same question.
- **When** I click the button
- **Then** the existing page is published (or brought back from its parked `/<callsign>/deleted-<id>`
  address) and no second page is made, so the address is never in conflict. A home page archived by
  moderation is not revived: I am pointed to the site tools instead.

### Owner Home State: `fn_main_owner_home` [@ANCHOR: user_websites:fn_main_owner_home]

`_owner_home(env_svc, profile_user, profile_group, slug, website_id)` in `controllers/main.py` is the single question
"what is at `/<slug>` for this owner?", asked both by the placeholder page and by `create_site`, so the two cannot
disagree (the bug found 2026-10-29: an unpublished home made the page offer "Create" and the button then failed on the
unique address). `env_svc` is the service-account environment; exactly one of `profile_user` and `profile_group` is a
non-empty record; `website_id` is the current website. It returns a pair `(page, state)` where state is one of the
constants `HOME_NONE`, `HOME_PUBLISHED`, `HOME_UNPUBLISHED`, `HOME_DELETED` (the owner deleted it and it is parked at
`/<slug>/deleted-<id>`, found through `deleted_original_url` and `deleted_by_owner`) or `HOME_BLOCKED` (archived by
moderation, or the address belongs to another owner: never reused or overwritten). With no page, `page` is an empty
recordset. Read-only; searches with `active_test=False`. Verified by `[@ANCHOR: test_create_site_reuses_existing_home]`.

### Writing a Blog Post
- **Given** I have an active site
- **When** I navigate to my blog index
- **Then** I can create my blog container, the first time
- **And** I can create a new blog post.
- **And** my followers will receive a weekly digest of my new content.

### Blog Index Route: `COMM_user_blog_index` [@ANCHOR: user_websites:COMM_user_blog_index]

`GET /<website_slug>/blog` and `/<website_slug>/blog/page/<int:page>` (`user_blog_index` in `controllers/main.py`, public,
`website=True`). Arguments besides the slug (`tag`, `search`, `date_begin`, `date_end`, `page`) are accepted for Odoo
compatibility; the page is not paged yet (the pager is an empty stand-in). It resolves the slug to a member or group
through the service account. An unknown slug first tries the retired-slug redirect (301 to the live site), then 404; a
hidden, ineligible or suspended member, or a suspended group, is 404. The owner (or any member of the group) sees the
blogs and all posts; everyone else sees published posts only (limit 100 each). With no blog and no posts it renders
`user_websites.placeholder_page` with `page_type = "blog"` (the owner gets the Create button), otherwise
`user_websites.blog_index`. Read-only. Verified in `tests/test_member_site_home_url.py` and `tests/test_qweb_context.py`.

### Create Blog Route: `COMM_create_blog` [@ANCHOR: user_websites:COMM_create_blog]

`POST /<website_slug>/create_blog` (`create_blog` in `controllers/main.py`; `auth="user"`, CSRF protected) makes the
caller's blog container the first time. The slug must be the caller's own member site, or a group the caller belongs to;
otherwise the response is 404, a suspended site is 404, and an ineligible member is redirected (303) to the owner tools.
It creates one `blog.blog` through the service account named by `_new_blog_name` ("<Name> Blog"), owned by the member or the
group, on the current website, and redirects to `/<slug>/blog`. The create passes through the blog quota check, so a member
at the limit gets a validation error instead of a second blog. It is not idempotent: repeating it makes another blog until
the limit or the unique name constraint (`name`, owner) stops it. Test: `tests/test_user_websites_blog_post.py`.

### Create Blog Post Route: `COMM_create_blog_post` [@ANCHOR: COMM_create_blog_post]

`POST /<website_slug>/create_blog_post` (also cited as `[@ANCHOR: UX_CREATE_BLOG_POST]` for the button and its tour) starts
a new post. Ownership checks match `create_blog`. If the owner has no blog yet it makes one with the same values, so the owner
toolbar's "New blog post" works from any page. It then creates a blog post named "New Post" in the owner's first blog through
the service account, setting `author_id` explicitly (the member's partner, or the requesting member's partner for a group)
so the byline is not "System Provisioner", and redirects to `/blog_post/edit/<id>`, the module's own ownership-scoped edit
form, because Odoo's builder toolbar needs the sitewide designer group that members must not hold. Returns 404 or a 303 as
for `create_blog`. Quota failures raise a validation error. Verified by `[@ANCHOR: test_tour_create_blog]`; the button is
the `UX_CREATE_BLOG_POST` tour target in `static/tests/tours/create_blog_tour.js`.

### Create-Blog-Post Button: `UX_CREATE_BLOG_POST` [@ANCHOR: UX_CREATE_BLOG_POST]

The blog index and the placeholder page show a form that posts (with a CSRF token) to `/<slug>/create_blog_post`; the
blog-page owner sees it even when the blog already exists, and it is how a first post is added once the container exists.
The help link next to the placeholder heading points at the documentation anchor `#UX_CREATE_BLOG_POST`. It is rendered only
to the owner. The tour `static/tests/tours/create_blog_tour.js` clicks it and expects the edit form to load, and
`tests/test_lifecycle_and_groups.py` checks the same path without a browser.

### Weekly Digest: `send_weekly_digest` [@ANCHOR: send_weekly_digest]

`blog.post.send_weekly_digest(self)` in `models/blog_post.py` (`@api.private`, `@api.model`) is what the weekly cron calls
(`ir_cron_send_weekly_digest`, see the technical foundation story). It takes no arguments and returns nothing. It reads
the next 50 rows after the stored cursor `user_websites.last_digest_id` from the `user_websites.weekly_digest_view` SQL
view (one row per subscriber and followed owner, listing the post ids of the week), makes an HMAC-SHA256 unsubscribe token
from the instance crypto secret, and queues one mail per subscriber with `List-Unsubscribe` and one-click post headers
through the mail service account (not sent inline). If the secret is missing it logs an error and sends nothing. A full
batch of 50 moves the cursor and triggers the cron again; a short batch resets it to 0, so the run is resumable and a repeat
after a crash resends at most one batch. Verified by `[@ANCHOR: test_weekly_digest_mail_template]`.

### Where My Blog Lives
- **Given** my site is `/<callsign>`
- **Then** my blog is at `/<callsign>/blog` and each post at `/<callsign>/blog/<post-slug>`; the blog's name is never part
  of an address, and a new blog is named "<Name> Blog" without a possessive.
- **And** every link the site makes to my blog and posts (the index, the owner dashboard, the post page's Read Next and
  sign-in links, the weekly digest, the Atom feed at `/<callsign>/blog/feed` and the sitemap) uses those addresses, read
  through the service account.
- **And** the old Odoo addresses (`/blog/<name>-<id>`, its pages, tags, feed and posts) answer a permanent redirect to the new
  ones; a blog that belongs to nobody keeps its stock addresses. An unknown, hidden or suspended site is a 404.

### Blog Public Address: `blog_blog_site_path` [@ANCHOR: user_websites:blog_blog_site_path]

`blog.blog.site_path` (`models/blog_blog.py`) is a non-stored computed `Char`, label "Public Address". For a blog owned by a
member or a group whose site has a slug it is `/<slug>/blog`; for any other blog (the platform's own, or an owner with no
slug) it is the empty string. `_compute_site_path` reads all owner slugs at once through `_user_site_slugs()` (one service
account read for the whole recordset). Used by the stock redirects, the sitemap, `SiteBlogUrl` and the templates. A slug
rename changes the value on the next read, and the cache-invalidation notices make stale edge copies expire. Verified by
`[@ANCHOR: test_user_blog_urls_use_the_callsign]` in `tests/test_user_blog_urls.py`.

### Post Public Address: `blog_post_website_url` [@ANCHOR: user_websites:blog_post_website_url]

`blog.post._compute_website_url` (`models/blog_post.py`) calls Odoo's own computation, then replaces `website_url` for every
post that has an owner slug with `/<site-slug>/blog/<post-slug>`, where the post slug is Odoo's `slug` of the post (title and
`-<id>` tail, so it names exactly one post even when two titles slugify alike). Posts of an owner without a slug keep the
stock address. Because everything reads `website_url`, the "View" links, the weekly digest, the Atom feed, the sitemap, search
results and the builder's buttons all get the one public address. Computed on read, no side effects, and the owner is read
through the service account so a visitor never needs access to the member's user row.

### Site Slug Reader: `fn_mixin_user_site_slugs` [@ANCHOR: user_websites:fn_mixin_user_site_slugs]

`_user_site_slugs(self)` in `models/user_websites_owned_mixin.py` returns a dictionary `{record id: owner's website_slug}` for
the records of the recordset that have an owner (`owner_user_id`, else `user_websites_group_id`) with a non-empty slug;
records without one are absent. It browses the same ids as the service account with `active_test=False` (archived records
still have an address), because neither the public user nor a portal member may read another member's `res.users` row and a
public page's address is computed for visitors. One service read for the whole set. Read-only; no locks. Used by
`blog.blog.site_path` and `blog.post.website_url`. Verified by `[@ANCHOR: test_user_blog_urls_use_the_callsign]`.

### Blog Owner Lookup: `COMM_user_blog_owner` [@ANCHOR: user_websites:COMM_user_blog_owner]

`UserBlogPages._blog_owner(self, website_slug, rest)` in `controllers/blog_urls.py` finds which site a blog address belongs
to. `website_slug` is the first path segment; `rest` is the remainder (such as `/blog/feed`) used to build a redirect for a
retired slug. It returns `(profile_user, profile_group, redirect)`: one of the first two is a non-empty record, or `redirect`
holds the 301 response for a retired slug (built by `_legacy_slug_redirect`). It raises `request.not_found()` (404) for an
unknown slug, for a member who is not publicly visible (hidden, ineligible), and for a suspended group. Lookups go through
the cached `get_record_by_slug` of the service account. No writes. Verified by `[@ANCHOR: test_user_blog_urls_use_the_callsign]`.

### User Blog Post Route: `COMM_user_blog_post` [@ANCHOR: user_websites:COMM_user_blog_post]

`GET /<uwslug:website_slug>/blog/<post_slug>` (`UserBlogPages.user_blog_post`; public, `sitemap=False`). It resolves the site
with `_blog_owner`, extracts the post id from the slug's `-<id>` tail (`ir.http._unslug`), and looks the post up as the
visitor, so an unpublished post is visible to its owner alone. It is 404 when the id is missing, the post is not found, or the
post belongs to another site. A request with a different spelling (upper case, an old title) gets a 301 to the canonical
`post.website_url`. It then renders Odoo's own post page through `WebsiteBlog().blog_post` and sets the page's `blog_url` to a
`SiteBlogUrl` and its `blogs` to this blog only, so no other member's blog is listed. Read-only apart from the stock view
counter. Verified by `[@ANCHOR: test_user_blog_urls_use_the_callsign]`.

### User Blog Feed Route: `COMM_user_blog_feed` [@ANCHOR: user_websites:COMM_user_blog_feed]

`GET /<website_slug>/blog/feed?limit=N` (`UserBlogPages.user_blog_feed`; public, `sitemap=False`) returns an Atom document
(`application/atom+xml`) of the site's published posts, newest first by `post_date`. `limit` is a string clamped to 1 to 50
(`FEED_POSTS_MAX`); a non-number gives 15. Unknown or hidden sites are 404 as for the index, a retired slug gets its 301, and a
site with no blog is 404. The blogs are found through the service account, but the posts are searched as the visitor, so
only published posts appear. Rendered by the `user_websites.user_blog_feed` template with the site name, base URL and
`html2plaintext` for summaries. Read-only, no locking. Verified by `[@ANCHOR: test_user_blog_urls_use_the_callsign]`.

### Sitemap Enumeration: `website_enumerate_user_blogs` [@ANCHOR: user_websites:website_enumerate_user_blogs]

`website._enumerate_pages(self, query_string=None, force=False)` in `models/website.py` is a generator of `{"loc", "name"}`
dictionaries used by the sitemap and the link picker. It runs Odoo's own enumeration and drops entries under the stock
`/blog/<name>-<id>` prefix of every member or group blog (those now redirect). Unless a `query_string` is given, it then
lists each published post at its real address plus one `/<callsign>/blog` index per site, skipping posts of members who are
not publicly visible and of suspended groups. At most 5000 posts (`SITEMAP_POSTS_MAX`) are read, through the service
account. Read-only. Verified by `[@ANCHOR: test_user_blog_urls_use_the_callsign]`.

### New Blog Name: `fn_main_new_blog_name` [@ANCHOR: user_websites:fn_main_new_blog_name]

`_new_blog_name(entity_name)` in `controllers/main.py` returns `"<entity_name> Blog"`, for the name of a member's or group's
first blog; `entity_name` is the member or group display name (string, any text). There is no possessive: an apostrophe in
a name once ended up in Odoo's blog address (`...-k6bp-s-blog-1`). The public address no longer uses the name, but the name
is also the source of SEO names and exports, which this keeps free of the character. Pure function, no I/O, cannot fail.
Callers: `create_blog` and the automatic container made by `create_blog_post`. Verified by
`[@ANCHOR: test_user_blog_urls_use_the_callsign]`.

### Stock Blog Redirect Group: `COMM_owned_blog_path` [@ANCHOR: user_websites:COMM_owned_blog_path]

`_owned_blog_path(blog)` in `controllers/blog_urls.py` returns `blog.site_path` (`/<callsign>/blog`) for a blog owned by a
member or group, and the empty string for no blog or a blog that belongs to nobody. It is the one test the four stock
redirect handlers use to decide whether Odoo's own address is retired. `blog` is a `blog.blog` record or an empty recordset;
no other type is expected. No I/O beyond the stored compute and no errors. Verified by
`[@ANCHOR: test_user_blog_urls_use_the_callsign]`.

### Stock Blog Index Redirect: `COMM_stock_blog_redirects` [@ANCHOR: user_websites:COMM_stock_blog_redirects]

`UserBlogRedirects.blog(self, blog=None, tag=None, page=1, search=None, **opt)` in `controllers/blog_urls.py` is a bare
`@http.route()` override of Odoo's blog index, so it keeps every stock route (`/blog`, `/blog/<blog>`, tag and page forms) and
adds a check first. For a blog with a `/<callsign>/blog` address it answers a permanent (301) local redirect to it, adding
`/page/<n>` when `page` is not 1 (a non-number page, such as `?page=abc`, counts as 1). Other blogs call the stock method
unchanged. No side effects. Verified by `[@ANCHOR: test_user_blog_urls_use_the_callsign]`.

### Stock Blog Feed Redirect: `COMM_stock_blog_feed_redirect` [@ANCHOR: user_websites:COMM_stock_blog_feed_redirect]

`UserBlogRedirects.blog_feed(self, blog, limit="15", **kwargs)` overrides the stock Atom feed route. For a member or group
blog it returns a 301 local redirect to `<site_path>/feed`, that is `/<callsign>/blog/feed`, so existing feed readers
follow the move; the `limit` is not carried over. Any other blog gets the stock feed unchanged. No database writes and no
locking. The redirect is permanent so search engines and feed readers replace the old address, and it is a local redirect, so the target can never leave this site. A member's feed address needs no tag or archive forms, which is why only the plain feed is mapped. See also `COMM_user_blog_feed`, the route that answers at the new address. Verified by `[@ANCHOR: test_user_blog_urls_use_the_callsign]`.

### Old Post-Edit Redirect: `COMM_stock_old_blog_post_redirect` [@ANCHOR: user_websites:COMM_stock_old_blog_post_redirect]

`UserBlogRedirects.old_blog_post(self, blog, blog_post, **post)` overrides the legacy `/blog/<blog>/post/<post>` address of
Odoo (the "post-edit" style link). When the blog has a `/<callsign>/blog` address, the response is a 301 local redirect to
`blog_post.website_url`, the post's own public address; otherwise the stock handler runs. Arguments are the records Odoo
resolved from the path, so a post that does not exist never reaches it. It is registered through a bare `@http.route()` on a subclass of `WebsiteBlog`, so Odoo keeps the stock URL patterns and only the handler changes; the member's own post address is served by `COMM_user_blog_post`. Nothing is written, and a request for a post that has no owner is left to the stock page. Verified by
`[@ANCHOR: test_user_blog_urls_use_the_callsign]`.

### Stock Post Redirect: `COMM_stock_blog_post_redirect` [@ANCHOR: user_websites:COMM_stock_blog_post_redirect]

`UserBlogRedirects.blog_post(self, blog, blog_post, tag_id=None, page=1, enable_editor=None, **post)` overrides Odoo's post
page `/blog/<blog>/<post>`. If either the blog in the path or the post's own blog has a `/<callsign>/blog` address it sends a
301 local redirect to `blog_post.website_url`; otherwise it passes every argument to the stock method. The check of both blogs
stops a post being reached through another blog's address. No side effects. Verified by
`[@ANCHOR: test_user_blog_urls_use_the_callsign]`.

### Post Page Link Builder, Init: `COMM_site_blog_url_init` [@ANCHOR: user_websites:COMM_site_blog_url_init]

`SiteBlogUrl(default_path)` in `controllers/blog_urls.py` is a small class that stands in for the stock page's `blog_url`
helper (a QueryURL). The constructor `__init__(self, default_path)` keeps one string, the blog's own address
(`post.blog_id.site_path`, possibly empty), as `default_path`. It does no I/O. `user_blog_post` puts an instance into the
page's render context so every breadcrumb, tag badge, archive and blog-list link made from it points at the site blog.
Verified by `[@ANCHOR: test_user_blog_urls_use_the_callsign]`.

### Post Page Link Builder, Call: `COMM_site_blog_url_call` [@ANCHOR: user_websites:COMM_site_blog_url_call]

`SiteBlogUrl.__call__(self, path=None, path_args=None, **kwargs)` is invoked by the stock post template as
`blog_url(blog=..., tag=..., date_begin=...)`. When a `blog` record is passed it returns that blog's `site_path`, or the stock
`/blog/<slug>` when the blog belongs to nobody; with no blog it returns `default_path`. `path`, `path_args` and every other
keyword (tag, dates) are accepted and ignored, because a member's blog has no tag or archive pages. Returns a string; never
raises on missing arguments. Verified by `[@ANCHOR: test_user_blog_urls_use_the_callsign]`.

### Editing and Publishing a Blog Post
- **Given** I just created a new blog post, or want to revise an existing one of my own (or one
  owned by a group website I belong to)
- **When** I open its dedicated edit form
- **Then** I see my post's current title and body, pre-filled, and a checkbox for whether it is
  published
- **When** I change the title and/or body and save
- **Then** my post is updated and I am redirected to its live page.
- **And** if I am not the post's owner (or a member of the group that owns it), I am redirected
  away instead of seeing or changing its content.
- This exists because the generic website-builder "Edit" toolbar is gated on
  `website.group_website_designer`, a global, sitewide website-editing permission a personal-site
  owner does not and structurally cannot hold -- granting it would let any owner edit any website
  content sitewide, not just their own post. This dedicated form gives the owner a real way to
  title, write, and publish their own post without that sitewide permission change.

### Blog Post Edit Form: `UX_BLOG_POST_EDIT_FORM` [@ANCHOR: user_websites:UX_BLOG_POST_EDIT_FORM]

The QWeb template `user_websites.blog_post_edit_form` (`views/user_websites_templates.xml`) is the form at
`/blog_post/edit/<id>`. It posts with a CSRF token to `/blog_post/edit/submit/<id>`. Fields: `name` (title, required, at most
120 characters), `content` (the rich editor, `user_websites.rich_editor`, with image upload to `/my/site/<slug>/image`) and
the checkbox `is_published`. It shows alert text for the errors `missing_fields` and `too_long`, shows what the member typed
after a refused save (`form_name`, `form_published`), and links to "Back to my site" and Delete. The page layout is the
site's own. Verified in `tests/test_user_websites_blog_post.py` (`test_owner_can_edit_own_blog_post`).

### Blog Post Edit Route: `COMM_blog_post_edit` [@ANCHOR: user_websites:COMM_blog_post_edit]

`GET /blog_post/edit/<int:post_id>` (`blog_post_edit` in `controllers/main.py`; `auth="user"`, `website=True`) shows the edit
form. It asks `_get_own_blog_post_for_edit(post_id)` for a post the caller may edit; if that returns False (not found, not
the owner, not in the owning group) the caller is redirected to `/my/home` and nothing is shown. Otherwise
`_render_blog_post_editor(post)` renders the form with the stored title and a sanitised copy of the body. Read-only.
The route takes no body and has no side effect, so it can be requested again freely. The form it renders posts to `COMM_blog_post_edit_submit`; the post-creation route sends a member here right after making a new post. The status is 200 for a normal render. A caller who is not signed in is sent to the login page by Odoo before the handler runs, because the route needs a user. The error codes `missing_fields` and `too_long` can also arrive as a query parameter and are shown by the form template. Verified by `[@ANCHOR: test_owner_can_edit_own_blog_post]` and `[@ANCHOR: test_blog_post_edit_denied_for_non_owner]`.

### Blog Post Edit Submit: `COMM_blog_post_edit_submit` [@ANCHOR: user_websites:COMM_blog_post_edit_submit]

`POST /blog_post/edit/submit/<int:post_id>` (`blog_post_edit_submit(self, post_id, name="", content="", is_published="")`;
`auth="user"`, CSRF protected). It applies the same ownership gate (redirect to `/my/home` on failure). A title that is empty
or over 120 characters, or a body over 200,000 characters (`MAX_BODY_CHARS`), re-renders the form with status 400 and the
member's own text. The body is passed through the strict allow-list sanitiser for everyone, administrators included; hostile
markup that was removed is also filed as an automated violation report. It then writes name, cleaned content and
`is_published` through the caller's own environment, so `blog.post.write` enforces ownership and the allowed-field list, and
redirects to the post's public address. Unpublishing is just saving with the checkbox cleared: an unticked box means `is_published` is false, and the Cloudflare purge and cache notice are sent by `blog.post.write`. The write is not locked; a second save simply overwrites the first (last write wins). The route does not change the owner or group, because `blog.post.write` refuses that for non-administrators. A CSRF token is required, so a link from another site cannot submit it. The sanitiser is `sanitize_member_html` in `models/member_html.py`. Verified by `[@ANCHOR: test_owner_can_edit_own_blog_post]`,
`[@ANCHOR: test_blog_post_edit_denied_for_non_owner]` and `[@ANCHOR: test_blog_post_edit_denied_for_non_member]`.

### Edit Gate: `COMM_get_own_blog_post_for_edit` [@ANCHOR: user_websites:COMM_get_own_blog_post_for_edit]

`_get_own_blog_post_for_edit(self, post_id)` in `controllers/main.py` decides whether the current request may edit a post.
`post_id` is an integer id. It finds the post as the service account (so a new unpublished post can be seen at all), and
returns False when it does not exist, or when the caller is neither its owner, a member of its owning group, nor an Odoo
administrator. Otherwise it returns the same post browsed in the caller's own environment, so every later read and write
still passes `blog.post`'s own access and ownership checks: this helper only decides whether the request proceeds. No writes.
Both the edit form and its submit call it first, so a single rule covers viewing and saving, and the answer for an unknown post and for a post that is not yours is the same (a redirect to `/my/home`), which does not reveal whether a post id exists. It is a method of the controller, not a route, and it is never called from outside this module. Verified by `[@ANCHOR: test_blog_post_edit_denied_for_non_owner]` and `[@ANCHOR: test_blog_post_edit_denied_for_non_member]`.

### Managing Content Quotas
- **Given** the administrator has set a global page limit
- **When** I attempt to create more pages than allowed
- **Then** the system enforces a quota check and prevents the creation of the new page with a helpful error message.

- **Given** the administrator has set a global blog limit
- **When** I attempt to create more blogs than allowed
- **Then** the system enforces a quota check and prevents the creation of the new blog with a helpful error message.

- **Given** the administrator has set a global blog-post limit
- **When** I attempt to create more posts than allowed
- **Then** the system enforces a quota check and prevents the creation of the new post with a helpful error message.

### Page Quota: `website_page_quota_check` [@ANCHOR: website_page_quota_check]

The page quota is checked in `website.page.create` (`models/website_page.py`) before pages are made. It is keyed on the
`owner_user_id` or `user_websites_group_id` in the values, not on who calls, so a direct RPC call gets no exemption. For a
member the limit is `res.users._get_page_limit()` (the member's own `website_page_limit`, or the system parameter
`user_websites.global_website_page_limit`, default 100); for a group it is that global limit applied to the group. It counts
existing pages and the pages in the same batch and raises `ValidationError` "You have reached your limit of N website pages."
when the total would exceed the limit. It reads counts through the service account. If the service account is not
provisioned it falls back to `sys_provisioner`. Verified by `[@ANCHOR: test_page_quota_limit]`
(`tests/test_page_limits.py`).

### Blog Quota: `user_websites_blog_quota_check` [@ANCHOR: user_websites_blog_quota_check]

`blog.blog._check_blog_quota(self, vals_list)` (`models/blog_blog.py`) is called from `create`. It adds up, per owner user
and per owning group, the blogs that exist plus those in the batch, and raises `ValidationError` ("You have reached your limit
of N blogs." or "This group has reached its limit of N blogs.") when a count passes the limit. The member limit is
`_get_blog_limit()`, the system parameter `user_websites.global_blog_limit` (default 5); a group uses the same global value,
because no per-group limit field exists yet (a product decision, to-do). Group-owned blogs are covered on purpose: an earlier
version checked only `owner_user_id`. No admin exemption. Counts come from `_read_group` as the service account.
Verified by `[@ANCHOR: test_blog_quota_limit]` (`tests/test_blog_limits.py`).

### Blog Post Quota: `user_websites_blog_post_quota_check` [@ANCHOR: user_websites_blog_post_quota_check]

`blog.post._check_blog_post_quota(self, vals_list)` (`models/blog_post.py`) is the same check for posts. The member limit is
`_get_blog_post_limit()`, system parameter `user_websites.global_blog_post_limit` (default 500); a group has the same global
limit as a whole (default 500, no per-group field yet). Existing posts and posts in the same batch are summed per owner and per
group; going over raises `ValidationError` "You have reached your limit of N blog posts." (or the group form). It matters
because each post also queues a Cloudflare purge and a cache notice, so unbounded RPC creation was a cost attack. No admin
exemption. Verified by `[@ANCHOR: test_blog_post_quota_limit]` (`tests/test_blog_limits.py`).

## Technical Notes
- Site creation uses a service account to bypass standard Odoo restrictions while maintaining strict ownership ([@ANCHOR: mixin_proxy_ownership_create]). Verified by `[@ANCHOR: test_mixin_ownership_validation]`.

- Page edits are restricted to the owner or administrators ([@ANCHOR: mixin_proxy_ownership_write]). Verified by `[@ANCHOR: test_mixin_ownership_validation]`.

- URL slugs are cached in Redis for high-performance routing ([@ANCHOR: slug_cache_invalidation]). Verified by `[@ANCHOR: test_slug_cache_invalidation]`.

## Creating Blog Posts

### Blog Post Create: `COMM_blog_post_create` [@ANCHOR: user_websites:COMM_blog_post_create]

Creating a blog post enforces the caller's real create access (a public user can no longer create a post
owned by the shared public account), checks that the owner is plausible and that the blog post quota for
the user or group is not exceeded, and, for ordinary users, drops every field outside a fixed allow-list
(title, content, publish flag, owner, group, blog, website, meta and SEO fields). The record is then
created through the module's service account, and the cache-invalidation notice and Cloudflare purge are
sent for the new post's URLs.

`blog.post.create(self, vals_list)` in `models/blog_post.py` (`@api.model_create_multi`) takes a list of value dictionaries
and returns the new recordset. Order of work: `check_access("create")` as the real caller, `_check_proxy_ownership_create`,
`_check_blog_post_quota`; then, for callers who are not `su`, system, module administrator or service account, unknown
keys are deleted, `content` goes through `html_sanitize`, and write access to the chosen blog is required. The create runs as
the service account with `mail_notrack`; if that account is missing it falls back to the caller. Afterwards it sends a
`blog.post` cache notice per blog index address and queues the Cloudflare purge. Errors: `AccessError`, `ValidationError`.
Verified by `[@ANCHOR: test_tour_create_blog]` and `test_group_blog_post_creation` (`tests/test_lifecycle_and_groups.py`).

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
  A guard test fails when a new static platform route is missing from that list, and the constraint also refuses any
  first segment of a route registered in the running database (`edge_routing.utils.reserved_slug_words`), so a route added later
  cannot be shadowed even before the list is updated.

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

## One site per member, eligibility and Delete (2026-10-13, append-only)

Bruce: "hams1 publishes two web sites for the user. One is bruce-perens-k6bp, and one is k6bp." and "There is no
'delete' button on web pages or blog posts."

- **Given** a member's site address changes (the slug is renamed), **when** the change is saved, **then** their pages
  and menu items move to the new address, a permanent 301 stays at the old one, redirect chains are flattened and a
  redirect that would shadow the new site is dropped. A page that would collide with one already at the new address is
  kept, unpublished, at `/<slug>/former-home`. Nothing is deleted. Before this, the pages stayed at the old address,
  which Odoo serves before it reads any redirect: a second site, and an owner toolbar whose Edit and Unpublish
  answered 404 because the page was not under the current slug. ([@ANCHOR: user_websites:fn_rehome_site]; migration
  0.7 repairs existing sites [@ANCHOR: user_websites:migrate_rehome_stale_site_pages]. Verified by
  `[@ANCHOR: test_site_slug_one_site]`.)
- **When** anyone opens a retired address, public or the owner's tools (`/my/site/<old>/...`), **then** they are
  redirected to the same URL under the live slug: 301 for a GET, 308 for a POST so a form left open across a rename
  still lands. ([@ANCHOR: user_websites:owner_redirect_legacy_slug], [@ANCHOR: user_websites:fn_main_legacy_slug_redirect].)
- **Slug policy.** `user_websites.slug_policy` is `name_callsign` (open source: the slug is made from the name) unless a
  deployment sets `callsign` and supplies the member's callsign through `_slug_identity()`. Under `callsign` a member
  has one address, never a name-derived one; with no callsign there is no site address yet; two members can never share
  one (an error, not a `-1` suffix). Existing slugs are never cleared. ([@ANCHOR: user_websites:COMM_res_users_slug_policy].)
- **Eligibility.** `_website_eligibility_gaps()` is the extension point for "who may have a website and blog"; open source
  returns none. A member with a gap has no public site (404 everywhere), their content is unpublished and marked, the
  owner's tools explain what is missing, new pages and posts are refused, and everything returns when the gap closes.
  Administrators are not exempt on the public site. ([@ANCHOR: user_websites:site_eligibility]. Verified by
  `[@ANCHOR: test_site_eligibility_hook]`.)
- **When** I press Delete on a page or blog post (dashboard, editor, owner toolbar), **then** I am asked "Delete this
  page? It will be removed from your site and can't be seen by visitors." (a dialog; a plain confirmation page without
  JavaScript); on confirming it disappears from my site, my lists and every public surface, and its address answers 404.
  The record is kept archived with a `deleted_by_owner` time: the platform retains data and the interface does not
  promise erasure. The home page cannot be deleted. ([@ANCHOR: user_websites:COMM_owner_delete].)
- **And** I can Restore from the collapsed Deleted list on the dashboard; the item comes back unpublished, at its old
  address if free. Restore is allowed because it recovers an accident and the data was never erased; a page archived by
  moderation is not in my list and cannot be restored by me. ([@ANCHOR: user_websites:COMM_owner_restore]. Verified by
  `[@ANCHOR: test_owner_delete_page_and_post]`.)
