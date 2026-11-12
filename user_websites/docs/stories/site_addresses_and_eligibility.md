# Story: A Member's Site Address, and Who May Have a Site

As a **Member**, I want one site address that follows me when it changes, so that old links keep working and my toolbar always points at the live site. As an **Operator**, I want to decide who may have a site without deleting anyone's work.

## One Site Per Member `[@ANCHOR: user_websites:site_slug_module]`

A site is every `website.page` whose address is `/<slug>` or starts with `/<slug>/`, plus the blog under `/<slug>/blog`. `models/site_slug.py` holds the rules for choosing a slug and for what moves when it changes. Renaming used to add a redirect and leave the pages at the old address, so the old address kept answering as a second site and the owner's toolbar pointed at pages that were not under the current slug. A rename now moves the pages and menu items to the new slug, keeps a page that would collide (unpublished, content retained, under a former-home style name), flattens redirect chains, and removes any redirect that would shadow the new site. The old address redirects with a 301.

* **Slug policy** `[@ANCHOR: user_websites:fn_slug_policy]`. The system parameter `user_websites.slug_policy` is `name_callsign` (the default: the slug is built from the record's name) or `callsign` (the slug is built from the member's callsign, which a deployment supplies through `ResUsers._slug_identity()`). The parameter is read straight from the database, not through the cached, whitelisted parameter reader, so a change takes effect at once. Anything other than `callsign` means the default. Under the callsign policy `ResUsers._apply_slug_policy()` `[@ANCHOR: user_websites:COMM_res_users_slug_policy]` gives a member exactly one address, the slug of their callsign (K6BP becomes `k6bp`, K6BP/P becomes `k6bp-p`), replaces an explicit name-style slug, keeps a member without a callsign siteless until one is on file, and never touches service accounts.
* **Service accounts** `[@ANCHOR: user_websites:fn_site_slug_is_service_account]`. A helper reads `is_service_account` with SQL, because the callers include provisioning and invitation flows whose account may not read that field, and returns false for no user at all.
* **Current slug** `[@ANCHOR: user_websites:fn_site_slug_current_slug]`. Another SQL read of the member's stored `website_slug`, used to tell whether a member already has a site address.
* **Uniqueness** `[@ANCHOR: user_websites:fn_assert_slug_free]`. Members and groups share one slug namespace. The check raises `ValidationError` ("The site address ... is already taken") unless no other member or group, archived ones included, holds the slug.
* **Retired slugs** `[@ANCHOR: user_websites:fn_legacy_slug_target]`. The only record of a retired slug is the 301 redirect rows `website.rewrite` already holds. The helper follows them for at most three hops, without loops, to the slug a member or group holds today, so rows written before the chains were flattened still lead to the live site. The public `/<slug>` route uses it `[@ANCHOR: user_websites:fn_main_legacy_slug_redirect]` to send a retired address to the same page of the new site, built from the stored slug and quoted so it is always a same-origin path.
* **One redirect hop** `[@ANCHOR: user_websites:fn_site_slug_next_hop]`. `_next_hop(svc, current)` takes the service-account environment and a slug and returns the slug that `/<current>` redirects to, or `None`. Only an active 301 row whose target is a single-segment path counts (a target with a trailing newline, a second segment or no leading slash is not a slug), and only the first such row is read. The retired-slug lookup calls it up to three times.
* **Escaping a prefix search** `[@ANCHOR: user_websites:fn_site_slug_like_escape]`. `_like_escape(text)` returns `text` with `\`, `%` and `_` each prefixed by a backslash, so a slug is matched literally in the `=like` searches that move a site's pages, menu items and redirects (a slug may contain `_`, which `LIKE` would otherwise treat as any character). It has no side effects.
* **Counting holders** `[@ANCHOR: user_websites:fn_site_slug_count]`. `_count(env, model, domain)` returns how many records of `model` match `domain`, archived ones included, read as the edge-routing service account because the member asking may not be allowed to see other members or groups. The uniqueness check calls it for `res.users` and for `user.websites.group`.
* **Where a deployment plugs in** `[@ANCHOR: user_websites:fn_users_slug_identity_fields]` `[@ANCHOR: user_websites:fn_users_slug_identity]`. `ResUsers._slug_identity_fields()` returns the tuple of field names the callsign policy builds the slug from, and `ResUsers._slug_identity(vals=None)` returns the text that is slugified (from `vals` if they carry it, else from the record), or `False` when the member has none. Open source returns `()` and `False`, so the callsign policy changes nothing there; hams.com overrides both with the callsign.
* **Applying the policy on create and write** `[@ANCHOR: user_websites:fn_users_apply_slug_policy]`. `ResUsers._apply_slug_policy(record, vals)` runs from `create()` (`record` is `None`) and from `write()` on one member. It changes nothing unless the policy is `callsign` and the account is not a service account. When the identity is known and `vals` touch the slug or an identity field, it checks the slug is free (`ValidationError` if another member or group holds it) and sets `vals["website_slug"]` to the slugified identity, in place. It returns `True` only when the member has no callsign yet and no slug, which tells the caller to stop the mixin deriving a slug from the name; every other path returns `False`. An explicit slug for a member with no callsign is kept.
* **Who may write the slug themselves** `[@ANCHOR: user_websites:fn_users_self_writeable_fields]`. `ResUsers.SELF_WRITEABLE_FIELDS` is Odoo's list of fields a user may change on their own record. This module adds `privacy_show_in_directory` and `website_slug` to the inherited list and nothing else; `website_ineligible` is deliberately not on it, so no member can lift their own hiding.

## Language Codes Are Not Slugs

The `/<slug>` routes see every one-segment path. Odoo's language handling only runs for a path no route matched, so a slug route that matched `/en_US/` or `/fr/blog` would hide the language prefix and answer 404. The route converter refuses a value that is an installed language's code, which makes werkzeug treat the rule as not matching, so those paths reach the language handling as before. The three entries below describe the converter class, its matching method and its registration; all live in `models/ir_http.py`.

### `class SiteSlugConverter(UnicodeConverter)` [@ANCHOR: user_websites:site_slug_converter]

The werkzeug route converter used by every `/<uwslug:website_slug>` route of this module. It behaves exactly like the stock string converter (one path segment, no slash) except that a value which is an installed language code is refused. It holds no state of its own and is created by werkzeug when the routing map is built; the list of language codes is looked up per request, so installing a language needs no restart. It exists because `/en_US/` once matched a slug route and broke the language-prefixed tours (found 2026-10-13). Tests: `tests/test_member_site_home_url.py` (language-prefixed paths still work).

### `to_python(self, value)` [@ANCHOR: user_websites:fn_site_slug_converter_to_python]

Called by werkzeug at match time with the path segment text `value` (str). When a request with an environment exists and `value.lower()` is one of `language_slugs(request.env)` (from `edge_routing.utils`), it raises werkzeug's `ValidationError`, which tells the router this rule does not match, so another rule or Odoo's language handling takes the path. Otherwise it returns what `UnicodeConverter.to_python` returns, the value unchanged. With no request (during routing-map construction) it never refuses. It reads the language list on every call, which is cheap because the helper caches it per environment; it has no side effects. Tests: `tests/test_member_site_home_url.py` lines for `/en_US/` and `/fr/blog`.

### `IrHttp._get_converters(cls)` [@ANCHOR: user_websites:fn_ir_http_get_converters]

Class method on the `ir.http` inheritance. Returns the stock converter dictionary with one extra entry, `"uwslug"` mapped to `SiteSlugConverter`, so route declarations may write `<uwslug:website_slug>`. No arguments. Odoo calls it once when it builds the routing map; nothing is cached by this module. Without this registration the routes using `uwslug` would fail to load. Test: `tests/test_member_site_home_url.py` checks that the name is registered and the converter is the module's own class.

## Reserved Words

Every first segment of a platform route is reserved, and the list is also derived from the live routing map. A slug claimed before that and equal to such a word is a site nobody can reach. The 0.6 migration `[@ANCHOR: user_websites:migrate_report_reserved_slug_collisions]` reads the member and group slugs, logs each one that is on the fixed reserved list at ERROR level and returns them, and changes nothing, since renaming moves a member's address and that is a decision for a person.

## Who May Have a Site

By default every account is eligible. A deployment narrows this by overriding the gaps method described first below. Administrators and internal users have no exemption on the public site. Ineligibility hides and never deletes: public routes answer 404, stored state is reconciled by the entries below, and the owner is told what is missing. The code is in `models/site_eligibility.py` (module docstring), with the page check in `models/ir_http.py`, the field in `models/user_websites_owned_mixin.py` and the owner message in `controllers/owner_site.py`. Test: `test_site_eligibility_hook` in `tests/test_site_eligibility_hook.py` covers all of them.

### The eligibility extension point [@ANCHOR: user_websites:site_eligibility]

The concept and module `models/site_eligibility.py`, which adds the field `res.users.website_ineligible` (Boolean, default False, readonly, indexed, `prefetch="user_websites"`, never written from the web and not in `SELF_WRITEABLE_FIELDS`). That field mirrors the last reconciled answer so SQL views (the directory, slug routing) can use it without calling Python. Open source ships with nothing required; hams.com overrides the gaps method to ask for a verified amateur who has accepted the current agreement. While ineligible, no new page, post, blog or image is accepted. Test: `tests/test_site_eligibility_hook.py`.

### `_website_eligibility_gaps(self)` [@ANCHOR: user_websites:COMM_website_eligibility_gaps]

The one method a deployment overrides. `ensure_one()` is called, so it takes a single member record. It returns a list with one `{"code": str, "message": str, "url": str}` dict for each missing requirement: `code` is a stable machine name, `message` is shown to the owner, `url` is where the member fixes it (empty string if nowhere). An empty list means eligible. This version returns `[]`. It must not write and should be cheap, since the public routes ask it for every request to a member's site. Test: `tests/test_site_eligibility_hook.py`.

### `_hams_website_eligible(self)` [@ANCHOR: user_websites:COMM_hams_website_eligible]

Returns True when the member currently has no eligibility gaps (`not self._website_eligibility_gaps()`). Takes one record (`ensure_one()`), no other arguments, no side effects, no database write. The name keeps the original hams.com spelling; open source uses it only through `_website_publicly_visible`. It is the live answer, not the stored `website_ineligible` mirror, which can lag by up to the cron interval. Test: `tests/test_site_eligibility_hook.py` overrides the gaps method and checks the answer flips.

### `_website_publicly_visible(self)` [@ANCHOR: user_websites:COMM_website_publicly_visible]

True when visitors may see this member's site right now: the member is not suspended from websites (`is_suspended_from_websites`) and `_hams_website_eligible()` is true. One record only (`ensure_one()`), no arguments, no writes. Both public routes and the page-serving hook call it, so a site is hidden the moment either condition fails, without waiting for the reconciler. It does not distinguish the reason; the owner's own tools do, through the suspended and ineligible responses below. Test: `tests/test_site_eligibility_hook.py`.

### `IrHttp._serve_page(cls)` for member pages [@ANCHOR: user_websites:fn_ir_http_serve_page_eligibility]

Class method override of Odoo's page-serving step in `models/ir_http.py`. Odoo's own page lookup knows nothing about owners, so a page under `/<slug>/...` would be served even while the member's site is hidden. This override takes the first path segment, lowercased; if it is not an installed language code it looks the member up by slug through the service account, and when that member exists and `_website_publicly_visible()` is false it returns `False` (the page is not served and Odoo answers 404). Anything else goes to the stock implementation. Read only, no locks. It closes the gap before the next reconcile. Test: `tests/test_site_eligibility_hook.py`.

### `reconcile_website_eligibility(self, user_ids=None)` [@ANCHOR: user_websites:COMM_reconcile_website_eligibility]

Brings stored state in line with the live eligibility answer. `@api.private` and `@api.model`, so it cannot be called from RPC. `user_ids` (list of ints, optional) restricts the work to those members; `None` means all members with a site, from `_website_site_user_ids()`. It runs as the user-websites service account. For each existing member it compares `bool(_website_eligibility_gaps())` with the stored `website_ineligible`: newly ineligible gets `_hide_site_content` and the flag set; eligible again gets `_show_site_content` and the flag cleared; still ineligible gets another hide pass to catch content published in a race. Returns `{user_id: "hidden" | "shown"}` for members that changed, and logs the dictionary at info level. Idempotent and safe to repeat. Called by the cron below, by deployment state-change hooks, by the owner dashboard and by the owner routes. Tests: `tests/test_site_eligibility_hook.py` and `tests/test_slug_resolution_cache_and_filters.py`.

### `hidden_by_eligibility` field [@ANCHOR: user_websites:field_hidden_by_eligibility]

A Boolean on the owned-record mixin (`models/user_websites_owned_mixin.py`, inherited by `website.page` and `blog.post`), string "Hidden While Ineligible", default False, not copied, indexed. It is set by the hide step when a published page or post is unpublished because its owner may not have a site, and cleared when the show step republishes it. It is what lets the reconciler republish exactly what it hid and never a piece the owner unpublished themselves. Only the reconciler writes it; any code that republishes content must leave marked pieces to the reconciler. Test: `tests/test_site_eligibility_hook.py`.

### `_cron_reconcile_website_eligibility(self)` and its cron record [@ANCHOR: user_websites:COMM_cron_reconcile_website_eligibility]

The scheduled entry point on `res.users` (`@api.model`): it calls `reconcile_website_eligibility()` with no arguments and returns its result dict. The cron record is `ir_cron_reconcile_website_eligibility` [@ANCHOR: user_websites:ir_cron_reconcile_website_eligibility] in `data/user_websites_data.xml`: named "User Websites: Hide and Show Sites by Eligibility", run as the user-websites service account, every 10 minutes, active, state `code`. A failure of one run changes nothing permanent and the next run repeats it. This is the safety net for a requirement lost without any hook firing. Tests: `tests/test_site_eligibility_hook.py` (both anchors).

### `_website_site_user_ids(self)` [@ANCHOR: user_websites:fn_eligibility_site_user_ids]

`@api.model` helper returning the ids (list of int) of the members the reconciler should look at when no list is given: active users whose `website_slug` is set and non-empty and who are not service accounts, since only those can have a site to hide. It uses one static SQL query with no parameters (marked audit-ignore-sql), so it is fast on a large user table and does not load records. No arguments, no side effects. Test: `tests/test_site_eligibility_hook.py`.

### `_hide_site_content(self, env_svc, user)` [@ANCHOR: user_websites:fn_eligibility_hide_site_content]

`@api.model` helper. `env_svc` is the service-account environment, `user` the member. It finds every published `website.page` (`website_published`) and `blog.post` (`is_published`) owned by `owner_user_id == user.id`, and writes unpublished plus `hidden_by_eligibility = True` in one write per model. Nothing is deleted and nothing else changes; run again with nothing published it does nothing, so it is safe to repeat. Returns nothing. Called only by the reconciler. Writing as the service account bypasses the owner's own rights on purpose. Test: `tests/test_site_eligibility_hook.py`.

### `_show_site_content(self, env_svc, user)` [@ANCHOR: user_websites:fn_eligibility_show_site_content]

`@api.model` helper, the inverse of hiding. If `user.is_suspended_from_websites` it returns at once and republishes nothing, because the suspension keeps the site hidden. Otherwise, using `env_svc`, it finds the member's pages and posts marked `hidden_by_eligibility`, sets them published and clears the mark. A page the member unpublished themselves was never marked and stays unpublished. Returns nothing; idempotent. Called only by the reconciler when a member becomes eligible again. Test: `tests/test_site_eligibility_hook.py`.

### The owner is told what is missing [@ANCHOR: user_websites:owner_site_ineligible]

A branch of `_resolve_site(self, site_slug)` in `controllers/owner_site.py`, which every owner-tools route calls. When the signed-in owner's `_website_eligibility_gaps()` is not empty it first runs `reconcile_website_eligibility([profile_user.id])` to hide anything still public, then raises `SiteResponse` carrying the template `user_websites.owner_site_ineligible` rendered with HTTP status 403 and the context `gaps` (the list of code, message, url), `site_slug` and `main_object`. The owner therefore sees what to fix and a link for each, and tools that create pages, posts or images are refused. Other members get 404 before reaching this branch. Test: `tests/test_site_eligibility_hook.py`.

## When an Owner-Site Route Must End Early `[@ANCHOR: user_websites:fn_answers_site_response]`

Resolving the owner's site can end the request with a response of its own: a redirect from a retired slug, or the "what is missing" page. Raising an HTTP exception for that would be turned by Odoo's frontend error handler into its own error page (a 301 becomes a 418), so the resolver raises `SiteResponse` instead, and the `answers_site_response` decorator on each owner route returns that response as the route's result.

## A Suspended Owner Is Told Why

See the entry below.

### A suspended owner is told why [@ANCHOR: user_websites:owner_site_suspended]

A branch of `_resolve_site(self, site_slug)` in `controllers/owner_site.py`. It is reached only by the signed-in owner of the site (another member gets 404 first, so nobody else learns the site exists). If the member `is_suspended_from_websites`, it raises `SiteResponse` with the `user_websites.owner_site_suspended` template, HTTP status 403, context `site_slug` and `main_object`. The page explains the suspension and where to appeal (the appeal form of the moderation story). The check runs before the eligibility check, so a suspended member sees the suspension, not a requirements list. Test: `tests/test_owner_site.py` (`test_owner_site_suspended_explains_and_points_to_appeal`).
