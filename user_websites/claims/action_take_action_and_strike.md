---
anchor: action_take_action_and_strike
code_hash: sha256:e0f38503923f782b206b4ecab194f2d59a79502ca68558fcd6cce5df607be631
---

# Claim: `ContentViolationReport.action_take_action_and_strike`

Written after an adversarial bug-hunt pass (`hams_shared/agents/skills/bug-hunt/SKILL.md`) that
traced this method's own state-transition against every place `content.violation.report.state` is
read or written across this file, `models/website_page.py` (both of this method's real callers),
and `views/content_violation_report_views.xml` (the only UI-level gate on it). Phrased in EARS,
extended with the FOR-EACH pattern. The statements below describe *verified* behavior, not the
behavior implied by the method's own docstring ("Enforces the 3-strike rule") or by the view's
`invisible` attribute, which a bug-hunt pass must not mistake for a server-side guard.

FOR-EACH `report` IN `self` (the recordset this method is invoked on, in Python iteration order;
no filtering, deduplication, or ordering guarantee is applied by this method itself):

1. THE system SHALL set `report.state` to `"action_taken"` unconditionally -- regardless of
   `report`'s state at the time this method is called (including if it is already `"action_taken"`
   or `"dismissed"`), and regardless of whether `report.content_owner_id` or
   `report.content_group_id` is set.
2. IF `report.content_owner_id` is truthy, THEN THE system SHALL, in order: call
   `self._increment_strike_count("res_users", owner.id)` (sibling function, out of this claim's own
   scope -- see `user_websites/claims/increment_strike_count.md` if it exists by the time this is
   read, otherwise treat as an external dependency); call
   `notify_model_invalidation(self.env, "res.users")`; call
   `owner.invalidate_recordset(["violation_strike_count"])`; IF
   `owner.violation_strike_count >= 3 AND NOT owner.is_suspended_from_websites`, THEN call
   `owner.action_suspend_user_websites()` (sibling function, out of scope); and THEN
   `message_post` an internal note on `report`, under service account
   `zero_sudo.mail_service_internal`, stating the owner's current strike count.
3. ELSE IF `report.content_owner_id` is falsy AND `report.content_group_id` is truthy, THEN THE
   system SHALL perform the equivalent group sequence: `_increment_strike_count("user_websites_group",
   group.id)`, `notify_model_invalidation(self.env, "user.websites.group")`,
   `group.invalidate_recordset(["violation_strike_count"])`, a conditional
   `group.action_suspend_group_websites()` call under the same `>= 3 AND NOT is_suspended_from_websites`
   guard, and a `message_post` under the same mail service account stating the group's current
   strike count.
4. IF `report.content_owner_id` is falsy AND `report.content_group_id` is falsy, THEN THE system
   SHALL NOT call `_increment_strike_count`, SHALL NOT call `message_post`, and SHALL NOT log or
   raise anything -- `report.state` is still set to `"action_taken"` per statement 1, giving no
   visible signal, anywhere, that no strike was applied to anyone. This is directly reachable in
   ordinary operation: `controllers/main.py`'s public report-submission route only sets
   `content_owner_id`/`content_group_id` at all when the reported URL's first path segment resolves
   via `get_record_by_slug` to a known user or group (lines ~78-86); a report on a URL with no slug
   (e.g. `/`) or an unrecognized slug is created with neither field set, and the admin backend form
   still shows the "Take Action & Strike User" button as available (`state == "new"`) for it.
5. IF `report.content_owner_id` AND `report.content_group_id` are BOTH truthy, THEN THE system
   SHALL apply a strike only per statement 2 (the owner) and SHALL NOT apply any strike per
   statement 3 (the group) -- the `elif` silently drops the group side. Not reachable today through
   any call site actually exercised in this codebase: the one path that could populate both fields
   at once (`website_page.py`'s `_trigger_malicious_arch_violation`, copying
   `vals.get("owner_user_id")`/`vals.get("user_websites_group_id")` onto
   `content_owner_id`/`content_group_id`) can only see both truthy if the *source* `website.page`/
   `blog.post` vals themselves have both set, which `user_websites_owned_mixin.py`'s own
   `_check_proxy_ownership_create` raises `ValidationError` on before `create()`/`write()` can
   complete -- rolling back the same transaction, including this method's own effects if it already
   ran earlier in that same request. This statement's real content is: `content.violation.report`
   has **no constraint of its own** (`_sql_constraints`/`models.Constraint`/`@api.constrains`)
   enforcing "at most one of `content_owner_id`, `content_group_id`" -- the only thing preventing
   this branch from firing today is an invariant enforced on a *different* model
   (`website.page`/`blog.post`, via the ownership mixin) and relied on transitively. A future
   direct write to `content.violation.report` (a migration, a new controller, an RPC integration)
   that sets both fields would hit this silently-wrong branch with nothing in this model's own
   schema to stop it.
6. THE system SHALL NOT check `report.state` before executing statements 1-5, anywhere in this
   method's own body (no such check exists), and -- traced across the full call chain rather than
   assumed from this method alone -- neither of its two real callers checks it either:
   `website_page.py`'s `_trigger_malicious_arch_violation` locates `existing` (the report to
   re-strike) by `(target_url, reported_by_user_id)` alone, with **no `state` filter**, so a report
   already `"action_taken"` or `"dismissed"` from a prior call is found again and handed back into
   this method unchanged. The only `state`-based gate anywhere in this feature is
   `content_violation_report_views.xml`'s button `invisible="... state in ('action_taken',
   'dismissed')"` -- a form-view convenience that governs whether an admin sees a clickable button
   in the backend UI, and has no effect on direct ORM calls, RPC calls, or either
   `website_page.py` call site (both of which invoke this method directly on a `with_user(svc_uid)`
   recordset, never through that view).
7. WHEN this method is invoked again on a `report` already processed by a prior call (per
   statement 6, directly reachable via `website_page.py`'s own re-strike-the-existing-report path
   on repeated `create()`/`write()` calls for the same `target_url` + `reported_by_user_id`), THE
   system SHALL re-execute statements 1-3 in full: `_increment_strike_count` runs again and adds a
   further strike on top of whatever was already applied, `report.state` is redundantly re-set to
   `"action_taken"`, and the `>= 3 AND NOT is_suspended_from_websites` suspension check is
   re-evaluated against the now-higher count -- with no version check, no "already processed"
   marker, and no dedup of any kind anywhere in this call chain. Concretely: a user whose page fails
   arch sanitization on the same URL across three separate saves accrues three strikes and
   suspension from three saves of one page, not from three distinct reports.

**ISOLATION: NOT fully maintained** -- confirmed by reading, not assumed. When two or more reports
in the same `self` recordset share the same `content_owner_id` (or the same `content_group_id`),
processing the first one's strike-and-possibly-suspend sequence is visible to the second's own
threshold check later in iteration order: `owner.invalidate_recordset(["violation_strike_count"])`
runs before the second report's own read of `owner.violation_strike_count`/
`owner.is_suspended_from_websites`, so the second report's guard sees the first report's
just-applied strike and (if the first already triggered suspension) correctly skips a second
`action_suspend_user_websites()` call via its own `NOT owner.is_suspended_from_websites` check. This
cross-report visibility is intentional and correct for the real-world semantics (multiple distinct
violation reports against the same target should accumulate toward one shared 3-strike counter, not
each start from zero) and is safe against lost updates given the stored procedure's own atomic
per-row lock (`FOR NO KEY UPDATE`, per `test_05_concurrent_strike_locking`'s own assertion). For two
reports in the same `self` targeting *different* owners/groups, no cross-item effect exists:
`_increment_strike_count`, `invalidate_recordset`, and `message_post` are each scoped to the
specific target row or report record, never to the batch as a whole. Not independently verified in
this pass (out of its own scope -- sibling functions): whether `action_suspend_user_websites`/
`action_suspend_group_websites` themselves have any effect (an unpublish/cascade side effect) that
could reach a still-unprocessed report or target later in the same `self` loop.

**Root-cause summary.** This method has zero idempotency/state guard of its own, and the one thing
that looks like a guard (the form view's `invisible` attribute) is UI-only and does not apply to
either of the method's two actual production callers -- both of which invoke it directly via the
ORM under a service-account user, bypassing the view entirely. Combined with
`website_page.py`'s own existing-report lookup being unfiltered by `state` (statement 6), the
realistic trigger is not a contrived double-click but ordinary repeated use: the same user's page
failing arch sanitization more than once accrues one strike per save, uncapped, rather than one
strike per distinct violation. Separately, statement 4's silent no-strike-but-still-"action_taken"
branch is reachable through the ordinary public report form whenever a reported URL's slug doesn't
resolve to a known user or group -- a state transition that visually reads as enforcement having
occurred (the kanban/list "Action Taken (Strike)" badge) when nothing was in fact struck.

**Test coverage note.** `# Verified by [@ANCHOR: test_moderation_suspension]` is accurate as far as
it goes: `tests/test_moderation.py::test_01_three_strikes_suspension` genuinely creates three
separate reports (each a fresh record, each with `content_owner_id` set), calls this method on each,
and asserts both the strike count reaching 3 and `is_suspended_from_websites` becoming true --
real coverage of the 3-strike suspension threshold, not a bug-class-3 case of claimed-but-absent
testing. `test_04_group_moderation_cascading_strikes` and `test_05_concurrent_strike_locking` add
real group-branch and lock-invocation coverage. None of the existing tests, however, exercise:
calling this method twice on the *same* report (statement 7), a report with neither
`content_owner_id` nor `content_group_id` set (statement 4), or a report already in
`"action_taken"`/`"dismissed"` state being passed back in (statement 6) -- these are the three
findings this claim exists to record, not gaps in the anchor's own narrower claim.
