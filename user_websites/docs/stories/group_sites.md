# Story: Group Websites

As a **Group Member**, I want to manage a shared website and blog for my team or club so that we can communicate our collective mission and updates.

Each behaviour is explained in full in the "Anchor reference" at the end of this file (ADR 0113); the entry names are given in square brackets.

## Scenarios

### Creating a Group Site
- **Given** I am a member of an Odoo group that has a linked `user.websites.group` record.
- **When** I navigate to the group's URL (e.g., `/my-team/home`).
- **Then** I see a placeholder page if the site hasn't been created yet [group home routing].
- **When** I click "Create Site", the system initializes the home page for the group [create site].
- **And** the group is assigned as the owner of the page.

### Shared Blogging
- **Given** the group has an active site.
- **When** any member of the group navigates to `/my-team/blog`.
- **Then** they can create new blog posts [create blog post].
- **And** all posts are attributed to the group's shared blog container.

## Technical Notes
- Group site routing follows similar logic to personal sites: both the personal and group `/home` fallback are served by the same controller method [group home routing].
- Access to edit or delete group pages is restricted to members of the associated Odoo group [ownership write check].
- Slugs for groups are also cached in Redis [group slug cache].
- Group members can be unsubscribed from notifications using a secure link [unsubscribe link].

## Anchor reference

### Create site: the "Create Site" button [@ANCHOR: UX_CREATE_SITE]

The user-facing action (help text `UX_CREATE_SITE` in `data/documentation.html`) and the controller `create_site(self, website_slug, **kwargs)`, `POST /<uwslug:website_slug>/create_site` in `controllers/main.py`, signed-in users only, CSRF checked. `website_slug` is the site's address; it must resolve to a member or a group. A non-owner, a suspended site and an unknown slug answer 404; a member not eligible for a site is redirected (303) to the owner tools page that says what is missing; a group site needs the user to be a member of the group. Then `_owner_home` decides: nothing (create), an unpublished home (publish it), a home the owner deleted (restore it at its address) or a blocked one (redirect with `error=home_blocked`). A new page is a published `website.page` at `/<slug>` named "<name> Home", built from `template_default_home`, with `user_websites_group_id` set for a group or `owner_user_id` for a member; the entity name is XML-escaped before it enters the template. Created as the service account. Returns a redirect to the page. Test: `[@ANCHOR: test_group_site_creation]`, `tests/test_lifecycle_and_groups.py::test_02_group_member_access`, which has a group member post to the group's create_site route and checks the group home page exists, owned by the group; the tour `create_site_tour.js` covers the button.

### Create blog post: the "Create Blog Post" button [@ANCHOR: UX_CREATE_BLOG_POST]

The user-facing action (help text `UX_CREATE_BLOG_POST` in `data/documentation.html`) and the controller `create_blog_post(self, website_slug, **kwargs)`, `POST /<uwslug:website_slug>/create_blog_post` in `controllers/main.py` (signed in, CSRF checked). It repeats the ownership rules of create site (member or group member only, 404 for suspended or foreign sites, 303 to the owner tools when ineligible), finds the owner's or group's blog container or creates one, then creates a blank `blog.post` named "New Post" in it and redirects to website_blog's own editing route. For a member the author is the member's partner; for a group the author is the requesting member's partner, so the byline credits whoever actually wrote it. Created as the service account (it would otherwise be credited to "System Provisioner"). Test: `[@ANCHOR: test_group_blog_post_creation]`, `tests/test_lifecycle_and_groups.py::test_08_group_inverse_relationships`, which creates a page and a post owned by a group and checks they appear in the group's `website_page_ids` and `blog_post_ids`; the tour `create_blog_tour.js` covers the button.

### Group home routing [@ANCHOR: user_websites:COMM_user_home_fallback]

`user_home_fallback(self, website_slug, **kwargs)`, `GET /<uwslug:website_slug>` in `controllers/main.py` (`auth="public"`, `sitemap=False`), the single method that serves both a member's and a group's home at `/<slug>`. Steps: look the slug up as a member and as a group through the service account; if neither, follow a retired-slug redirect, then Odoo's own page lookup (so stock pages such as `/aboutus` keep working), then 404; a hidden member site (suspended or not eligible) or a suspended group is 404; a slug in other letter case is redirected (301) to the canonical slug. If the home page exists and is published, a Redis view counter is incremented (not for the administrator; a Redis failure is only logged) and Odoo's fallback serves it without the page cache. Otherwise the placeholder page `user_websites.placeholder_page` is rendered with `is_owner` true for the member or any group member, which is what shows the "Create Site" button. Test: `[@ANCHOR: test_group_site_routing]`, defined in `tests/test_lifecycle_and_groups.py::test_02_group_member_access`, and `test_member_site_home_url`.

### Ownership write check [@ANCHOR: mixin_proxy_ownership_write]

`_check_proxy_ownership_write(self, vals)` on the mixin `user_websites.owned.mixin` (`models/user_websites_owned_mixin.py`), called from `write()` of `website.page`, `blog.post` and `blog.blog`. `vals` is the dictionary being written. A non-administrator may not change `owner_user_id` or `user_websites_group_id`: if either key is in `vals` it raises `AccessError` ("You cannot transfer ownership..."), so a member or group member cannot hand a record to someone else. Superuser environments, system administrators, user-websites administrators and the ownership-bypass group may transfer, but even they get `ValidationError` if the result would have both a user and a group as owner. Returns nothing. Edits of other fields are not restricted here; who may edit a group's page at all is decided by the record rules. Test: `[@ANCHOR: test_mixin_ownership_validation]`, `tests/test_sdk_extensibility.py::test_03_mixin_ownership_validation`, in which an intruder is refused when creating a page owned by another user; the mutual-exclusion case is `test_05_api_armor_mutual_exclusion`.

### Group slug cache [@ANCHOR: group_slug_cache_invalidation]

The distributed cache behind `user.websites.group.get_record_by_slug(slug)`: `user.websites.group` inherits `edge.routing.mixin`, whose `write()` computes old and new slugs and sends a cache invalidation notification (`_notify_cache_invalidation("user.websites.group", slugs)`) after the change, so every worker drops the stale mapping when a group's slug changes; `unlink()` does the same (the `group_slug_cache_invalidation_unlink` entry in the README). Cache keys are per model name, so member and group slugs never collide. A failed notification is logged and does not block the write. See the module README for the full entry. Test: `[@ANCHOR: test_group_slug_cache_invalidation]`, `tests/test_audit_edge_cases.py::test_05_bdd_ormcache_query_counting_group_slugs`, which creates a group, primes the cache, checks that a second lookup runs no `user_websites_group` SQL and that after the slug is changed the new slug resolves.

### Unsubscribe link [@ANCHOR: user_websites:COMM_unsubscribe]

`unsubscribe(self, model, record_id, partner_id, timestamp, token, **kwargs)`, `GET /website/unsubscribe/<model>/<record_id>/<partner_id>/<timestamp>/<token>` in `controllers/main.py` (`auth="public"`, `csrf=True`), the link in digest mails. `model` must be `res.partner` or `user.websites.group` and the record must exist, else JSON `{"error": "Forbidden"}` with status 403. The token must equal the HMAC-SHA256, keyed by the database crypto secret, of `model-record_id-partner_id-timestamp`, compared in constant time; a missing secret or a wrong token also answers 403 with the same body, so nothing is revealed. On success it removes the partner from the record's followers (`message_unsubscribe([partner_id])`) as the service account and renders `unsubscribe_success`. The timestamp is part of the signature but this handler does not compare it with the clock, so a link does not expire (to-do: decide whether it should). Tests: `[@ANCHOR: test_unsubscribe_secret]`, defined in `tests/test_subscriptions.py` in the weekly-digest test, which sends the digest, finds the `List-Unsubscribe` and `List-Unsubscribe-Post` headers, opens the unsubscribe link as a public user (success page "Unsubscribed Successfully") and checks the follower was removed; and `test_02_invalid_unsubscribe_token`, which sends a forged token, gets 403 and checks the follower is still subscribed.
