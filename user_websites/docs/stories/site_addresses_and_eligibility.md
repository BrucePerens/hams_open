# Story: A Member's Site Address, and Who May Have a Site

As a **Member**, I want one site address that follows me when it changes, so that old links keep working and my toolbar always points at the live site. As an **Operator**, I want to decide who may have a site without deleting anyone's work.

## One Site Per Member `[@ANCHOR: user_websites:site_slug_module]`

A site is every `website.page` whose address is `/<slug>` or starts with `/<slug>/`, plus the blog under `/<slug>/blog`. `models/site_slug.py` holds the rules for choosing a slug and for what moves when it changes. Renaming used to add a redirect and leave the pages at the old address, so the old address kept answering as a second site and the owner's toolbar pointed at pages that were not under the current slug. A rename now moves the pages and menu items to the new slug, keeps a page that would collide (unpublished, content retained, under a former-home style name), flattens redirect chains, and removes any redirect that would shadow the new site. The old address redirects with a 301.

* **Slug policy** `[@ANCHOR: user_websites:fn_slug_policy]`. The system parameter `user_websites.slug_policy` is `name_callsign` (the default: the slug is built from the record's name) or `callsign` (the slug is built from the member's callsign, which a deployment supplies through `ResUsers._slug_identity()`). The parameter is read straight from the database, not through the cached, whitelisted parameter reader, so a change takes effect at once. Anything other than `callsign` means the default. Under the callsign policy `ResUsers._apply_slug_policy()` `[@ANCHOR: user_websites:COMM_res_users_slug_policy]` gives a member exactly one address, the slug of their callsign (K6BP becomes `k6bp`, K6BP/P becomes `k6bp-p`), replaces an explicit name-style slug, keeps a member without a callsign siteless until one is on file, and never touches service accounts.
* **Service accounts** `[@ANCHOR: user_websites:fn_site_slug_is_service_account]`. A helper reads `is_service_account` with SQL, because the callers include provisioning and invitation flows whose account may not read that field, and returns false for no user at all.
* **Current slug** `[@ANCHOR: user_websites:fn_site_slug_current_slug]`. Another SQL read of the member's stored `website_slug`, used to tell whether a member already has a site address.
* **Uniqueness** `[@ANCHOR: user_websites:fn_assert_slug_free]`. Members and groups share one slug namespace. The check raises `ValidationError` ("The site address ... is already taken") unless no other member or group, archived ones included, holds the slug.
* **Retired slugs** `[@ANCHOR: user_websites:fn_legacy_slug_target]`. The only record of a retired slug is the 301 redirect rows `website.rewrite` already holds. The helper follows them for at most three hops, without loops, to the slug a member or group holds today, so rows written before the chains were flattened still lead to the live site. The public `/<slug>` route uses it `[@ANCHOR: user_websites:fn_main_legacy_slug_redirect]` to send a retired address to the same page of the new site, built from the stored slug and quoted so it is always a same-origin path.

## Language Codes Are Not Slugs `[@ANCHOR: user_websites:site_slug_converter]`

The `/<slug>` routes see every one-segment path. Odoo's language handling only runs for a path no route matched, so a slug route that matched `/en_US/` or `/fr/blog` would hide the language prefix and answer 404. The route converter `SiteSlugConverter` refuses a value that is an installed language's code, which makes werkzeug treat the rule as not matching, so those paths reach the language handling as before. `to_python()` `[@ANCHOR: user_websites:fn_site_slug_converter_to_python]` does the check at match time, so installing a language needs no restart, and the converter is registered under the name `uwslug` by the `ir.http` override `[@ANCHOR: user_websites:fn_ir_http_get_converters]`.

## Reserved Words

Every first segment of a platform route is reserved, and the list is also derived from the live routing map. A slug claimed before that and equal to such a word is a site nobody can reach. The 0.6 migration `[@ANCHOR: user_websites:migrate_report_reserved_slug_collisions]` reads the member and group slugs, logs each one that is on the fixed reserved list at ERROR level and returns them, and changes nothing, since renaming moves a member's address and that is a decision for a person.

## Who May Have a Site `[@ANCHOR: user_websites:site_eligibility]`

By default every account is eligible. A deployment narrows this by overriding `ResUsers._website_eligibility_gaps()` `[@ANCHOR: user_websites:COMM_website_eligibility_gaps]`, which returns one `{"code", "message", "url"}` dict per missing requirement and an empty list when the member is eligible. `_hams_website_eligible()` `[@ANCHOR: user_websites:COMM_hams_website_eligible]` is true when the list is empty, and `_website_publicly_visible()` `[@ANCHOR: user_websites:COMM_website_publicly_visible]` is true when the member is both eligible and not suspended from websites. Administrators and internal users have no exemption on the public site.

Ineligibility hides and never deletes:

* Public routes ask the live answer and answer 404. A page under a member's slug is not served while the site is hidden `[@ANCHOR: user_websites:fn_ir_http_serve_page_eligibility]`, because Odoo's page lookup knows nothing about owners; this closes the gap before the next reconcile.
* `reconcile_website_eligibility()` `[@ANCHOR: user_websites:COMM_reconcile_website_eligibility]` brings stored state in line with the live answer. For a newly ineligible member it unpublishes the member's pages and posts and marks each `hidden_by_eligibility` `[@ANCHOR: user_websites:field_hidden_by_eligibility]`, and it sets the read-only mirror `res.users.website_ineligible`. For a member who is eligible again it republishes exactly the marked pieces, and never a page the member unpublished themselves, nor anything while the member is suspended. It returns `{user_id: "hidden" | "shown"}` for the members that changed and is safe to repeat.
* It runs from a scheduled job every ten minutes, `_cron_reconcile_website_eligibility()` `[@ANCHOR: user_websites:COMM_cron_reconcile_website_eligibility]`, installed as `[@ANCHOR: user_websites:ir_cron_reconcile_website_eligibility]` and run as the user-websites service account. It also runs from the state-change hooks a deployment supplies and lazily from the owner's dashboard.
* The owner is told what is missing instead of getting a 404 `[@ANCHOR: user_websites:owner_site_ineligible]`: the owner's tools answer 403 with the list of requirements and a link to fix each, hide anything still public, and refuse to create new pages, posts or images while ineligible.

## When an Owner-Site Route Must End Early `[@ANCHOR: user_websites:fn_answers_site_response]`

Resolving the owner's site can end the request with a response of its own: a redirect from a retired slug, or the "what is missing" page. Raising an HTTP exception for that would be turned by Odoo's frontend error handler into its own error page (a 301 becomes a 418), so the resolver raises `SiteResponse` instead, and the `answers_site_response` decorator on each owner route returns that response as the route's result.

## A Suspended Owner Is Told Why `[@ANCHOR: user_websites:owner_site_suspended]`

When a member's site is suspended, the owner's tools answer 403 with a page that explains the suspension and where to appeal. Nobody else learns that the site exists.
