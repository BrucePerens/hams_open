---
anchor: user_websites:COMM_appeal_action_approve
code_hash: sha256:258b1eb4baf79dc607338f580231498763272bde87fae2234c49a7fce4489ac3
---

# Claim: `ContentViolationAppeal.action_approve`

Written after an adversarial bug-hunt pass (`hams_shared/agents/skills/bug-hunt/SKILL.md`) that
checked this method against its sibling `action_reject` (reviewed separately, see
`user_websites/claims/appeal_action_reject.md`) for the same two questions: is there a server-side
state guard, and does the code actually re-verify the `_check_appeal_target` invariant it depends
on. Phrased in EARS, extended with the FOR-EACH pattern.

`action_approve` originally carried no base anchor of its own -- only a `# Verified by
[@ANCHOR: user_websites:test_tour_moderation_appeal]` citation, which is a rule-3 verification link,
not a rule-1 base declaration (confirmed: the citation's own tour never authenticates as admin or
calls `action_approve`; see bug-class finding 1 below). The orchestrating session added the missing
base anchor (`# [@ANCHOR: user_websites:COMM_appeal_action_approve]`, matching the sibling
`action_reject`'s existing naming convention), a matching `# Tests [@ANCHOR: ...]` citation on
`test_appeals_and_views.py::test_02_submit_and_approve_appeal` (the test that actually exercises
this function), and a `docs/modules/user_websites.md` entry, so this claim has a real anchor to
attach to and `verify_anchors.py`/`check_claims_freshness.py` both pass clean.

FOR-EACH `appeal` IN `self` (a `content.violation.appeal` recordset; the standard admin backend
form-view button (`content_violation_appeal_views.xml`) only ever invokes this on a single record
via `type="object"`, but the method's own `for appeal in self:` loop imposes no cardinality limit --
any caller obtaining a multi-record recordset (XML-RPC, a server action, a scheduled job, a future
list-view bulk-action wiring) can invoke it on more than one appeal in a single call):

1. WHEN `action_approve` is invoked, THE system SHALL set `appeal.state = "approved"`
   unconditionally, regardless of `appeal.state`'s value at call time -- there is no read of
   `appeal.state` anywhere in this method's own body (confirmed by reading; no `if appeal.state ==
   "new"` guard, no `filtered()` call before the loop). Calling this method again on an appeal
   already in state `"approved"` re-executes every following statement rather than being a no-op.
2. IF `appeal.group_id` is truthy, THEN THE system SHALL call
   `appeal.group_id.action_pardon_group_websites()` (which, for each group in that call, resets
   `violation_strike_count` to the literal `0`, sets `is_suspended_from_websites` to the literal
   `False`, calls `notify_model_invalidation(self.env, "website.page")`, and posts a new,
   undeduplicated `mail.thread` message on the group under service account
   `user_websites.user_websites_service_account`); ELSE (based solely on `bool(appeal.group_id)`,
   with no independent check of `appeal.user_id`) THE system SHALL call
   `appeal.user_id.action_pardon_user_websites()`, an exact structural mirror scoped to the user,
   under service account `zero_sudo.mail_service_internal`.
3. IF `appeal.group_id` is falsy AND `appeal.user_id` is ALSO falsy (both empty -- contrary to
   `_check_appeal_target`'s intended invariant that exactly one of the two is always set, reviewed
   separately), THEN `appeal.user_id.action_pardon_user_websites()` SHALL execute as a true no-op:
   Odoo/Python's own `for user in self:` performs zero iterations over an empty `res.users`
   recordset, no exception is raised, and control returns normally. `appeal.state` is still set to
   `"approved"` per statement 1, and the message in statement 5 is still posted with the "you
   pardoned the user" text, so THE system SHALL report and record an approval while pardoning
   nobody. Verified directly by reading Odoo's recordset iteration semantics and this method's own
   lack of any truthiness check on `appeal.user_id` before the call -- contingent on whether a real
   bypass of `_check_appeal_target` actually exists (that constraint is reviewed separately, under
   its own anchor `user_websites:COMM_check_appeal_target`, and its own claim found no live bypass
   today; this statement documents what `action_approve` itself would do if the invariant it trusts
   were ever violated, not that the invariant currently IS violated).
4. Re-invoking `action_pardon_user_websites()` / `action_pardon_group_websites()` on a target
   already at its pardoned defaults (strike count already `0`, already not suspended) SHALL NOT
   restore or alter any prior state and SHALL NOT double-decrement or double-accumulate anything --
   both methods write fixed literal values, not deltas or increments. Each such re-invocation SHALL,
   however, post one additional, undeduplicated "you pardoned this user/group" message to that
   target's own chatter; THE system provides no mechanism to suppress or merge repeated pardon
   messages for the same target.
5. WHEN the branch in statement 2 completes (successfully, or as the no-op described in statement
   3), THE system SHALL call `appeal.with_user(mail_svc).message_post(...)` on the appeal itself,
   choosing one of exactly two fixed message bodies based solely on `bool(appeal.group_id)` at the
   time this line executes -- not on whether the pardon call actually changed any field.
6. THE system SHALL NOT catch any exception raised while resolving `mail_svc` (before the loop
   begins, via `_get_service_uid("zero_sudo.mail_service_internal")`), or while executing the pardon
   call, or while executing `message_post` inside the loop (no `try`/`except` exists anywhere in
   this method's body). A failure while processing appeal N of a multi-appeal call aborts the `for`
   loop entirely: every appeal at index >= N (in whatever order `self` iterates) is left unprocessed
   for that invocation, while every appeal at index < N has already had `state = "approved"` written
   in the same, not-yet-committed ORM transaction (rolled back together with the rest of that
   transaction if the caller's own transaction rolls back on the propagated exception -- not
   independently durable per appeal, and not re-attempted automatically).
7. IF `_get_service_uid` cannot resolve `zero_sudo.mail_service_internal` to an active,
   correctly-flagged service account, THEN THE system SHALL raise (fail loudly), before the loop
   over `self` begins, for every appeal in the batch equally -- matching the sibling
   `action_reject`'s own statement 4.

**ISOLATION: partially maintained -- state-correct, side-effect-duplicating.** Verified by reading,
not assumed:

(a) `content.violation.appeal` carries no unique constraint on `user_id` or `group_id` (grepped the
model and `user_websites_security.xml`; none exists), and `/website/submit_appeal`
(`user_websites/controllers/main.py`) never checks for an existing `"new"` appeal on the same
target before creating another. So two (or more) appeals in the same `self` recordset CAN
legitimately share the same `user_id` or `group_id` -- e.g. a suspended user who submits an appeal,
receives no timely response, and submits a second one while still suspended; both remain in state
`"new"` simultaneously and could later be selected into the same batch call.

(b) When two appeals in the same call share a target, each loop iteration still independently sets
its OWN appeal's `state` to `"approved"` and posts its OWN appeal-level chatter message (statements
1 and 5) -- neither reads nor depends on anything a sibling iteration wrote. No appeal's own state
transition or own chatter message is corrupted, skipped, or altered by processing a sibling appeal
first. **Per-appeal state isolation holds.**

(c) The shared target's real-world side effects do NOT hold isolated, however: per statement 4,
`action_pardon_user_websites()` / `action_pardon_group_websites()` fires once per appeal naming that
target, so a batch containing N appeals for the same user/group produces N redundant "you pardoned
this user/group" chatter messages and N redundant (no-op-after-the-first) strike-count resets,
instead of one. This is a side-effect-duplication issue, not data corruption: the target's final
field values (`violation_strike_count == 0`, `is_suspended_from_websites == False`) are correct and
identical no matter how many of that target's appeals get approved together or separately.

(d) Statement 6's failure propagation is itself a real cross-item effect this FOR-EACH's own
isolation does not hold against: one appeal's processing failure (e.g. its `group_id`/`user_id`
target was deleted between appeal creation and this call, or `message_post` raises) prevents every
appeal later in iteration order from being processed at all in that invocation -- even though those
later appeals are otherwise entirely independent of the one that failed and may name unrelated
targets.

## Bug-class findings from this pass

1. **The function's own `# Verified by [@ANCHOR: user_websites:test_tour_moderation_appeal]`
   citation is false.** `user_websites/static/tests/tours/moderation_appeal_tour.js` submits an
   appeal (fills the reason textarea, clicks submit, asserts the form is replaced by a "reviewing
   your appeal" pending state) and never authenticates as an admin, never opens the backend appeal
   record, and never calls or triggers `action_approve` in any way -- confirmed against
   `test_ui_tours.py::test_04_moderation_appeal_tour`, the only Python test that runs this tour.
   Real coverage of `action_approve` exists, but under a different anchor entirely:
   `test_appeals_and_views.py::test_02_submit_and_approve_appeal` (now also tagged with this
   function's own real `# Tests [@ANCHOR: user_websites:COMM_appeal_action_approve]` link), which
   does authenticate as admin and call `appeal.action_approve()` -- plus a non-discriminating
   stochastic call in `test_simulation.py`. **Classified as bug class 3** ("Claimed test/
   verification coverage that doesn't actually exist") -- the pre-existing verification link on this
   function pointed at a test that verifies appeal *submission*, not appeal *approval*. Neither
   existing test exercises the group_id branch, the double-approval re-entry case, or a multi-appeal
   same-target batch. The stale `# Verified by [@ANCHOR: user_websites:test_tour_moderation_appeal]`
   line itself was left in place (not this pass's anchor scope to retarget or remove) -- flagged
   here so a future pass fixes or drops it.

2. **Bug class 18** (state-machine transition guard exists only in client-rendered UI, no
   server-side enforcement): statement 1 above -- no `if appeal.state != "new": raise` exists
   anywhere in this method; the only guard (`invisible="not id or state != 'new'"` on the form
   button) is UI-only, bypassable via RPC, a server action, a stale browser tab, or a double-click
   race before Odoo's own button-disable takes effect. This function's concrete instance:
   re-approving an already-approved appeal re-calls
   `action_pardon_user_websites()`/`action_pardon_group_websites()` and re-posts the "Appeal
   approved" note, producing duplicate audit-trail messages with no data corruption (final field
   values are idempotent, per statement 4 / ISOLATION (c) above). The sibling `action_reject`
   (`user_websites/claims/appeal_action_reject.md`) independently found the same missing-guard shape
   from the opposite direction, where re-invocation is NOT merely redundant: rejecting an
   already-`"approved"` appeal overwrites `state` back to `"rejected"` and posts a message asserting
   the user "remains suspended" that is provably false at that moment. Both are the same bug class,
   confirmed from two independent directions on sibling methods in the same file -- no new class
   needed.

3. Statement 3 above (blind trust in `_check_appeal_target`'s invariant, with a silent no-op +
   false-success outcome if it's ever violated) matches bug class 5 ("Silent-failure gates") in
   shape -- a dependency (the constraint holding) failing to be met would silently produce a
   no-op with no user-visible signal, rather than an error. The sibling claim
   `user_websites/claims/check_appeal_target.md` independently confirmed no live bypass of that
   constraint exists today (Python-only enforcement, but no `create()`/`write()` override, no raw
   SQL writer, and `_validate_fields` runs under `sudo()` unconditionally so a caller's own
   `sudo()` doesn't weaken it) -- so this is a latent, not live, gap: real today only if some future
   change adds a bypass route to that constraint.
