---
anchor: mixin_proxy_ownership_write
code_hash: sha256:ee4bc9b7d2fb28330264bd3a919990e6a2cd5c8729f1eb79b762fe2e881bfef6
---

# Claim: `UserWebsitesOwnedMixin._check_proxy_ownership_write`

Written after an adversarial bug-hunt pass (`hams_shared/agents/skills/bug-hunt/SKILL.md`) that
deliberately batched this function with its sibling `_check_proxy_ownership_create`
(`user_websites/claims/check_proxy_ownership_create.md`), the natural place to check the two
entry points enforcing the same "Proxy Ownership Pattern" invariant actually agree with each
other. This file carries the full create-vs-write consistency analysis; see the create claim for
create's own statement-by-statement behavior. Phrased in EARS, extended with FOR-EACH and a
mandatory ISOLATION clause.

**Real callers, confirmed by grep.** Not `@api.model` and not called automatically by any
framework hook -- a plain instance method, invoked explicitly. Grepping the whole module finds it
called as the second statement of each real inheritor's own `write()` override (immediately after
that override's own `self.check_access("write")` call, and before switching `self` to the
privileged service-account user for the real `super().write()` call): `website.page.write()`
(`user_websites/models/website_page.py:619-620`), `blog.blog.write()` (`blog_blog.py:139-140`),
`blog.post.write()` (`blog_post.py:218-219`). Unlike `create()`, no controller in this module calls
`.write()` on these models at all (grepped `controllers/main.py`) -- every real call to this
function today runs bound to the actual logged-in caller's own `self.env`, never to the service
account, so (unlike the create-side finding in the sibling claim) there is no live case today of
this function's own `is_admin` branch being reached via a service-account-mediated call whose real
human identity has already been laundered away.

1. IF `self.env.su` OR `self.env.user.has_group("base.group_system")` OR
   `...has_group("user_websites.group_user_websites_administrator")` OR
   `...has_group("user_websites.group_user_websites_service_account")` (the privileged branch),
   THEN, IF `"owner_user_id" in vals OR "user_websites_group_id" in vals`:

   FOR-EACH `record` IN `self` (the recordset `write()` was called on):

   1a. THE system SHALL compute `new_owner` as `vals.get("owner_user_id", <record's own current
       owner_user_id.id, or False>)` and `new_group` analogously for `user_websites_group_id` --
       each record's own computed values depend only on `vals` (shared across the whole FOR-EACH)
       and that SAME record's own pre-write field values, never another record's.
   1b. IF `new_owner` and `new_group` are both truthy, THEN THE system SHALL raise
       `ValidationError` ("cannot be owned by both a user and a group simultaneously").

   THE system SHALL then `return` -- but only if the FOR-EACH loop above completes without raising
   (or if the outer key-presence condition was False to begin with, so the loop never ran at all).
   If any record's own iteration raises `ValidationError` per 1b, that exception propagates
   immediately out of this function instead, and `return` is never reached for that call. Either
   way, a privileged caller never reaches statement 2 below -- confirmed by reading: statement 2's
   own condition is only evaluated when statement 1's own outer `if` was False.

2. IF the caller is not privileged (statement 1's own condition is False) AND
   `"owner_user_id" in vals OR "user_websites_group_id" in vals`, THEN THE system SHALL raise
   `AccessError` ("cannot transfer ownership of a record to another user or group") --
   **unconditionally on mere key presence**: this check does not read either key's own value, does
   not compare it to the record's current value, and does not distinguish a genuine ownership
   change from a no-op resubmission of the record's own current, unchanged value. **For a
   non-privileged caller, this is the ONLY check this function performs** -- there is no
   auto-assign, no mandatory-ownership check, and no per-record loop at all on this branch.

**ISOLATION: per-record state is isolated; the privileged branch's own loop reads only same-record
state (statement 1a) and no record's outcome depends on another's, matching the pattern already
confirmed in the sibling `action_approve` claim's own ISOLATION (b). The non-privileged branch has
no loop at all, so isolation is trivially maintained there (nothing to isolate against).**

## Cross-function consistency: does write() actually mirror create()'s invariants?

This is the reviewing session's own Q1 and Q4, worked precisely rather than eyeballed.

**Q1 -- can a non-admin `write()` strip both ownership fields to falsy, something create() would
never allow?** NO, and not because write() carries an explicit mirror of create()'s own mandatory-
ownership check (statement 7 in the create claim) -- it carries no such check at all. The reason is
structural: statement 2 above rejects a non-admin's write() outright if `vals` contains **either**
key, for **any** value -- setting it, nulling it, or leaving it byte-for-byte identical to the
record's current value. A non-admin therefore can never successfully write to `owner_user_id` or
`user_websites_group_id` at all, so they can never strip either one to falsy; the write-side rule is
not "weaker" than create's, it is differently shaped and, for this specific question, strictly
**stronger** -- create's rule permits a non-admin to legitimately set `owner_user_id` to themselves
or a group they belong to (a real write to the field succeeds); write's rule forbids a non-admin
from touching either field at all, successful or not. Verified against `tests/test_orm_security.py
::test_06_prevent_ownership_transfer` (lines 195-228), which exercises exactly this branch on both
`website.page` and `blog.post` -- real coverage that exists but is not currently cited by either
function's own `# Verified by` comment (worth adding in a future pass; not this claim's own anchor
to edit).

**Minor friction noted, not a confirmed defect and not a clean fit for any of the 19 existing bug
classes:** because statement 2 is a pure key-presence check, a non-admin whose own client
resubmits a form that happens to include their own current, unchanged `owner_user_id` (e.g. a
"save all fields" UI pattern) would be rejected with `AccessError` even though nothing would
actually change. Not verified live against any real caller in this codebase today (none of the
three real `write()` overrides appear to unconditionally forward every field from a form), so this
is a latent UX rough edge, not a demonstrated live bug.

**What about an admin?** An admin (or the service account) CAN strip a record to having neither
owner nor group via `write()` -- statement 1's own loop only checks the dual-ownership case
(1b), never the ownerless case. This is NOT an asymmetry with create(), though: create()'s own
mandatory-ownership check (statement 7 in the sibling claim) is skipped for admins too (`if
is_admin: continue`, before that check). Both entry points agree: an admin may leave a record
ownerless at creation time (by explicitly setting one field falsy and the other unset -- statement
4's auto-assign in the create claim otherwise fills `owner_user_id` for any non-public caller) and
may later strip it to ownerless via write(). **Consistent by design, not a bug.**

**Q4 -- is create's dual-ownership check (unconditional, even for admins) consistent with write's
own admin-only dual-ownership check, and does either make the other dead code?** No, neither makes
the other dead, and the asymmetry in *where* the check lives (create: outside any admin
conditional; write: inside the privileged branch only) is not an inconsistency once traced fully:

- In `write()`, a non-admin can never reach ANY dual-ownership evaluation, because statement 2's own
  unconditional `AccessError` fires first on mere key presence -- there is no scenario where a
  non-admin's `vals` both touches an ownership field AND survives to be checked for dual ownership.
  So write's dual check (1b) existing *only* inside the privileged branch is not an oversight: it is
  the only branch a dual-ownership *evaluation* can structurally ever reach. This matches its own
  docstring precisely: "prevents admins from creating dual-owned corrupted states" -- it was never
  meant to guard non-admins, who are blocked earlier and more broadly.
- In `create()`, by contrast, statement 5 (in the sibling claim) sits BEFORE the `is_admin: continue`
  branch specifically because create() has no equivalent blanket early exit for non-admins the way
  write() does -- a non-admin's `create()` call is expected to legitimately reach this point with a
  real owner or group value, so the dual check has to apply to everyone, admin and non-admin alike,
  at the one place both populations pass through.
- Both checks are real and reachable for an admin specifically: an admin's `create()` call with both
  fields set hits the create-side check (sibling claim statement 5); an admin's later `write()` call
  re-introducing both fields on an existing record hits this file's own statement 1b. Neither
  shadows the other -- they guard the same invariant at two different points in a record's
  lifecycle (creation vs. a later mutation), which is exactly the "check they actually agree with
  each other" question this dispatch was framed around, and they do.

## Bug-class findings from this pass

1. **Class 3 (false verification citation) -- confirmed false, unlike the sibling's own two
   citations.** This function's own `# Verified by [@ANCHOR: test_mixin_ownership_validation]`
   comment is FALSE. `test_03_mixin_ownership_validation`
   (`tests/test_sdk_extensibility.py:101-138`) -- the test that anchor name actually points to --
   calls `self.env["website.page"].with_user(intruder).create(...)` and nothing else; it never
   calls `.write()` anywhere in its body, and cannot exercise this function at all. Confirmed by
   reading the full test body, not by trusting the citation. This function's OTHER citation, `#
   Verified by [@ANCHOR: test_api_armor_mutual_exclusion]`, IS accurate:
   `test_05_api_armor_mutual_exclusion` (same file, lines 162-204) does call
   `page.write({"user_websites_group_id": test_group.id})` on an admin-context page whose
   `owner_user_id` is already set, genuinely exercising statement 1b's dual-ownership check ("Must
   prevent dual ownership on write, even for admins"). Real, uncited coverage of this function's
   OTHER branch (statement 2, the non-admin blanket denial) exists at
   `tests/test_orm_security.py::test_06_prevent_ownership_transfer` (lines 195-228) -- worth adding
   a `# Verified by` link for in a future pass, not this claim's own scope to edit the source
   comment.

2. This pass's own new candidate bug class (an `except` clause catching a Python exception type a
   real Postgres-level `RAISE EXCEPTION` failure never actually produces) does not apply to this
   function -- `_check_proxy_ownership_write` calls no `_get_service_uid` and has no `try`/`except`
   at all. See the sibling create claim's finding 2 for the full writeup and the corroborating
   `pager_duty/models/pager_check.py` comment.

No other mismatch between this function's stated docstring ("Prevents malicious actors from
spoofing or transferring ownership after creation, and prevents admins from creating dual-owned
corrupted states") and its actual behavior was found -- both halves of that docstring are accurate
against statements 1-2 above.
