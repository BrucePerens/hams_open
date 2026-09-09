---
anchor: mixin_proxy_ownership_create
code_hash: sha256:4394adc25d6735073bf7b772f2ac419eee1316becd4756b0edb3b731556b7381
---

# Claim: `UserWebsitesOwnedMixin._check_proxy_ownership_create`

Written after an adversarial bug-hunt pass (`hams_shared/agents/skills/bug-hunt/SKILL.md`) that
deliberately batched this function with its sibling `_check_proxy_ownership_write`
(`user_websites/claims/check_proxy_ownership_write.md`) because the two enforce the same
"Proxy Ownership Pattern" invariant from the `create()` and `write()` entry points respectively --
the natural place to check they actually agree. The write claim carries the full create-vs-write
consistency analysis (Q1/Q4 of the reviewing session's own brief); this file states create's own
behavior precisely and cross-references that analysis rather than duplicating it. Phrased in EARS,
extended with FOR-EACH and a mandatory ISOLATION clause.

**Real callers, confirmed by grep.** The mixin itself defines no `create()` override and no
`@api.model_create_multi` -- `_check_proxy_ownership_create` is `@api.model` only, a plain helper.
Grepping the whole module for `_inherit = [..., "user_websites.owned.mixin"]` finds exactly three
real concrete inheritors, and grepping for calls to this method finds that all three call it as the
first access-control statement of their own `create()` override -- literally the first statement
for `blog.blog.create()` (`blog_blog.py:65`) and `blog.post.create()` (`blog_post.py:125`);
preceded only by non-authorization input-shaping steps (a `name`-from-`view_id` backfill and an
XSS-sanitization pass over `arch`/`arch_base`/`arch_db`, neither of which reads or checks
ownership) for `website.page.create()` (`user_websites/models/website_page.py:398`) -- and in every
case before any field whitelist or quota check, and before switching `self` to the privileged
service-account user for the real `super().create()` call. So every real inheritor's `create()`
does run this check, bound to the real caller's own `self.env`, before any actual persistence or
ACL-relevant step -- but this is true only because each of the three remembered to call it
explicitly; nothing on the mixin itself would force a fourth inheritor to.

Ubiquitous statements (apply once per call, not per `vals` item):

1. THE system SHALL compute `is_admin` once, from the real calling user's own `self.env.user`, as
   `self.env.su OR has_group("base.group_system") OR has_group
   ("user_websites.group_user_websites_administrator") OR has_group
   ("user_websites.group_user_websites_service_account")`.
2. IF the union of `vals.get("user_websites_group_id")` truthy values across the WHOLE `vals_list`
   is non-empty AND `is_admin` is False, THEN THE system SHALL attempt, once for the whole batch
   (not once per item), to resolve `user_websites.user_websites_service_account`'s own uid via
   `_get_service_uid` and browse the requested `user.websites.group` records under that uid,
   populating `valid_group_members` (a `{group_id: set(member_user_ids)}` map consulted by every
   `vals` item's own group-membership check, statement 9).
3. IF that resolution raises `odoo.exceptions.AccessError`, THEN THE system SHALL log a debug
   message and `return` immediately -- **from the entire method, not just the group-resolution
   step** -- before the FOR-EACH loop over `vals_list` below ever begins. See ISOLATION and finding
   2 below for whether this can actually happen for the scenario its own comment names.

FOR-EACH `vals` IN `vals_list` (each dict mutated in place; iterated in the caller's own list
order):

4. WHEN `vals` has neither a truthy `owner_user_id` nor a truthy `user_websites_group_id`, AND
   `self.env.user._is_public()` is False, THE system SHALL set `vals["owner_user_id"]` to the real
   calling user's own id. (`_is_public()`, `odoo/addons/base/models/res_users.py:1173`, is
   `self.sudo().has_group('base.group_public')` -- true only for the special shared "Public User"
   pseudo-account Odoo binds to a genuinely anonymous/unauthenticated visitor; a `base.group_portal`
   user is NOT public by this definition and IS auto-assigned.)
5. IF, after statement 4, `vals` has both a truthy `owner_user_id` AND a truthy
   `user_websites_group_id`, THEN THE system SHALL raise `ValidationError` --
   **unconditionally, before any admin check**: this `if` (source line 98) is textually and
   executionally prior to the `is_admin: continue` branch (source line 105), so it applies to an
   admin, the service account, and an ordinary user identically. Verified directly by reading the
   statement order, not inferred from the surrounding comments.
6. WHEN `is_admin` is True, THE system SHALL perform no further check on that `vals` item (a bare
   `continue`) -- statements 7-9 below apply only when `is_admin` is False.
7. IF, after statement 4, `vals` has neither a truthy `owner_user_id` nor a truthy
   `user_websites_group_id`, THEN THE system SHALL raise `AccessError`. **This branch has exactly
   one real population that can ever reach it**: statement 4's own auto-assign already guarantees
   `owner_user_id` is truthy for every caller whose `_is_public()` is False and who didn't supply a
   group id themselves -- admin or not, service account or not. So statement 7 can only fire for a
   caller whose `_is_public()` is True (which also implies `is_admin` is False; the public
   pseudo-user is never a member of any admin group in this codebase). Verified by tracing every
   combination of the two booleans through statements 4-7, not assumed from the "must assign an
   owner" error text alone -- this directly answers this pass's own Q2.
8. IF, after statement 4, `owner_user_id` is truthy and `int(owner_user_id) != self.env.user.id`,
   THEN THE system SHALL raise `AccessError` ("cannot create a record owned by another user") --
   reachable only for non-admin callers (statement 6 already returned for admins).
9. IF, after statement 4, `user_websites_group_id` is truthy, THEN THE system SHALL raise
   `AccessError` unless that group's id is a key of `valid_group_members` (i.e. statement 2's batch
   resolution found it to exist) AND the real calling user's id is in that group's own member set
   -- reachable only for non-admin callers.

**ISOLATION: NOT fully maintained across `vals_list` -- confirmed by reading, not assumed.**

(a) Per-item statements 4-9, once the loop begins, read and write only that item's own `vals` dict
plus the read-only `valid_group_members` map built once before the loop starts -- no item's own
outcome is altered by what a sibling item's own processing wrote. **Per-item field mutation is
isolated**, matching the pattern's own `# ADR 0078: O(1) Memory Mapping` batching intent.

(b) Statement 3 is a real cross-item leak, however: a service-account/group-resolution failure
triggered by *any single* item's own `user_websites_group_id` (or a group id shared by several
items) silently aborts validation for **every** item in the batch, including items specifying no
group at all that would otherwise be validated or rejected independently. A batch mixing one
group-owned page with several ordinary owner-owned pages, where the group lookup fails, skips
statements 4-9 entirely for all of them -- no auto-assign, no dual-ownership check, no
mandatory-ownership check, no owner-identity check, no group-membership check -- and every `vals`
item proceeds straight to the real DB insert with nothing but the earlier `is_admin` computation
having run against it.

## Bug-class findings from this pass

1. **Class 3 (false verification citation) -- on the sibling, not here.** This function's own
   citations are both accurate: `# Verified by [@ANCHOR: test_mixin_ownership_validation]`
   (`test_03_mixin_ownership_validation`, `tests/test_sdk_extensibility.py:101-138`, really does
   call `.create()` and exercise statement 8's "owned by another user" branch) and `# Verified by
   [@ANCHOR: test_api_armor_mandatory_assignment]` (`test_06_api_armor_mandatory_assignment`,
   same file lines 206-231, exercises statement 4's auto-assign and statement 9's `AccessError` for
   a nonexistent group id). The sibling `_check_proxy_ownership_write`'s own citation of
   `test_mixin_ownership_validation`, however, is FALSE -- see
   `user_websites/claims/check_proxy_ownership_write.md`.

2. **New candidate bug class (proposed in this pass's report, NOT added to SKILL.md here per this
   session's own instruction that the orchestrating session lands shared-file edits): a
   `try/except` catches an exception type the real failure path never actually raises**, because
   the real failure is a raw SQL/PL-pgSQL `RAISE EXCEPTION` surfacing as a `psycopg2`-level
   exception, not the wrapped Python domain exception the comment describes. Statement 3's own
   `except AccessError` cannot catch `_get_service_uid`'s real "not found / disabled / is a human
   user, not a service account / has admin groups" failure modes -- all four are raised via
   `RAISE EXCEPTION` inside the `zero_sudo_get_service_uid` Postgres function
   (`zero_sudo/data/postgres_procedures.xml:35,42,46,56`), a database-level exception, not
   `odoo.exceptions.AccessError`. `_get_service_uid`'s own Python-level `AccessError`
   (`zero_sudo/models/security_utils.py:57-60`) fires only for a malformed `xml_id` string, which
   this call site's hardcoded literal (`"user_websites.user_websites_service_account"`) can never
   produce. **Independently corroborated by an unrelated module's own comment describing the
   identical defect for the same underlying function**: `pager_duty/models/pager_check.py:250-259`
   --- "Resolving the account first (the old order) hit an uncaught Postgres exception from
   `zero_sudo_get_service_uid()` when `binary_downloader` wasn't installed, since that's not a
   Python ValueError/UserError/AccessError any of this method's except clauses catch -- it also
   aborts the current DB transaction." Concretely: the "Defer if service account is not yet
   provisioned during early testing" deferral this comment promises **does not happen** for the
   scenario it names -- a genuinely missing or misconfigured service account raises an uncaught,
   transaction-poisoning Postgres exception instead of being gracefully deferred, through this
   exact call site, today. This is reachable in production, not just "early testing": a service
   account row deleted, deactivated, or stripped of its `is_service_account`/group flags by a bad
   migration or an admin mistake would hit the same uncaught path. Candidate linter check: flag a
   Python `except <SpecificOdooException>` wrapping a call chain that bottoms out in a bare
   `self.env.cr.execute("SELECT <plpgsql_function>(...)")` whose own `CREATE FUNCTION` body (or the
   XML file defining it) contains `RAISE EXCEPTION` with no corresponding Python-side translation
   -- a real false-positive risk (many such calls are legitimately guarded by something else), so
   flag for manual review rather than hard-fail, matching this skill's own precedent for
   speculative candidates (see SKILL.md's "Candidates not yet implemented" section).

3. **Class 5 (silent-failure gate), latent given finding 2's own conclusion that statement 3's
   `except` almost never actually fires for its stated scenario.** IF it ever does catch a genuine
   `AccessError` (a future refactor of `_get_service_uid`, or some other Python-level `AccessError`
   -- note the `for group in groups: if group.exists(): ...` loop that follows is NOT inside this
   `try`, only the `_get_service_uid` call and the lazy `.browse()` construction are), the resulting
   `return` is a silent, `_logger.debug`-only skip of every per-item validation in the whole batch
   (ISOLATION (b) above), not just the group-membership check its own placement and comment
   suggest.

4. **Related to Q2, a factual claim in a test's own docstring does not match the code's real
   behavior (class 3/6-adjacent, on a test comment rather than a doc or a checker).**
   `test_07_api_armor_public_user_must_have_owner`
   (`tests/test_sdk_extensibility.py:233-258`) calls `_check_proxy_ownership_create` directly
   (bypassing `create()`) and its own docstring reasons: "website.page's own ACL already denies
   `base.group_public` any create right at all, so this path can't be reached through `create()`
   as a real anonymous visitor." Tracing the real `create()` call chain shows this reasoning is
   backwards: `website.page.create()` (and `blog.blog`/`blog.post`) never calls
   `self.check_access("create")` against the real caller (only `write()`/`unlink()` do, explicitly,
   at their own top) -- the ORM's own built-in ACL check for `'create'`
   (`odoo/orm/models.py:4646`, `self.check_access('create')` inside the base `create()`) only runs
   once the override has already switched `self` to `self_svc = self.with_user(svc_uid)` for the
   final `super(..., self_svc).create(vals_list)` call, so it is checked against the **service
   account's** own permissions, which are full (`ir.model.access.csv`:
   `access_website_page_svc,...,1,1,1,1`), not against `base.group_public`'s row
   (`access_website_page_public,...,1,0,0,0`, `perm_create=0`) at all. That row is never actually
   consulted for this model's real `create()` code path. So a genuinely public `self.env.user`
   calling `env["website.page"].create(vals)` directly (e.g. via generic JSON-RPC, if any route
   ever exposed it that way) WOULD reach this function's own body with the original public
   identity, and WOULD hit statement 7's `AccessError` if `vals` supplied neither owner nor group --
   not because the ACL stopped it first, but because `_check_proxy_ownership_create` itself is the
   real, sole, load-bearing gate. The test's actual assertion is still correct and the security
   property it probes still holds -- just via a different mechanism than the docstring states. This
   also generalizes: the same ACL-bypass-via-service-account pattern means `base.group_portal`'s own
   `perm_create=0` row for these three models is likewise never consulted against a real portal
   caller's own `create()` call -- the ENTIRE create-time access-control burden for
   `website.page`/`blog.post`/`blog.blog` rests on this function (statements 4-9), not on
   `ir.model.access`, matching this pattern's own stated purpose ("let ordinary non-admin users
   'own' records that a service account actually writes to the DB on their behalf") rather than
   being a bug -- but it does mean the mixin's own two functions are the actual security boundary,
   not a defense-in-depth backstop, and this pass recommends fixing the misleading docstring
   reasoning in a future pass (out of this claim's own anchor scope to edit).

**Design observation, not a confirmed defect (nothing forces a future inheritor to call this
method):** because the mixin defines no `create()`/`write()` override of its own, a fourth model
that inherits `user_websites.owned.mixin` but forgets to call `_check_proxy_ownership_create` in
its own `create()` override would get zero ownership enforcement at create time, silently. All
three real inheritors today do call it correctly (confirmed above) -- flagging for awareness, not
as a live bug against the two anchors reviewed here.
