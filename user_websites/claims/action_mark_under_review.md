---
anchor: user_websites:action_mark_under_review
code_hash: sha256:6a957caa3b3dbdb5e999594515880cf59e5375f86da9ffe2bd7d063890faec1f
---

# Claim: `ContentViolationReport.action_mark_under_review`

Written after an adversarial bug-hunt pass (`hams_shared/agents/skills/bug-hunt/SKILL.md`)
reviewing this method together with the sibling `action_dismiss` (`claims/action_dismiss.md`) --
both trivial one-line state-setters raising the identical question: is any state-machine
transition actually enforced, anywhere, or does `write()` accept any value from any prior state?
Phrased in EARS. Not phrased with the FOR-EACH extension -- see "Why not FOR-EACH" below.

1. WHEN `action_mark_under_review()` is called on recordset `self` (one or more
   `content.violation.report` records), THE system SHALL set `state = "under_review"` for every
   record in `self`, via the single ORM call `self.write({"state": "under_review"})`.
2. IF any record in `self` has a `state` other than `"new"` at the time of the call --
   `"under_review"` (already), `"action_taken"`, or `"dismissed"` -- THEN THE system SHALL still
   set that record's `state` to `"under_review"`. No check of a record's own prior `state` exists
   anywhere in this method's body, in the `state` field's own definition (`fields.Selection`, no
   `@api.constrains`), or in any `models.Constraint`/`_sql_constraints` on this model (verified by
   reading the full model: only `_report_uniq`, `_url_not_empty`, and `_desc_not_empty` exist, none
   touching `state`). No other module extends `content.violation.report`'s `write()` (verified by
   `grep -rln '_inherit.*content.violation.report'` across the codebase returning no matches).
3. THE system SHALL NOT raise, log, message-post, or otherwise surface any signal when statement
   2's condition is true -- a report already `"action_taken"` (meaning a strike has already been
   permanently applied to `content_owner_id`/`content_group_id` via the
   `increment_strike_count()` stored procedure, and a suspension may have already fired) or already
   `"dismissed"` is silently reopened to `"under_review"`, indistinguishable in the record's own
   state from a genuinely fresh report.
4. IF the calling user lacks `write` access on `content.violation.report` -- `base.group_portal`
   and `base.group_public` are both `perm_write=0` in `security/ir.model.access.csv` -- THEN THE
   system SHALL raise `odoo.exceptions.AccessError` when `write()` executes, per the ORM's standard
   access path. This is the only real enforcement boundary this method has: a *who may call it*
   check (satisfied by membership in `user_websites.group_user_websites_administrator` or
   `user_websites.group_user_websites_service_account`, both `perm_write=1`), not a *may this
   transition happen* check.
5. THE system SHALL NOT be gated, beyond client-side widget rendering, by the form view's
   `invisible="not id or state != 'new'"` condition on this button
   (`views/content_violation_report_views.xml:75`). That condition is evaluated in the browser to
   decide whether to draw the button; it does not run server-side and does not restrict what
   `action_mark_under_review()` itself will accept. Any caller with model write access -- a direct
   RPC/`call_kw` invocation, an already-rendered stale form (loaded while the record was still
   `"new"`, submitted after another moderator has since moved it to `"action_taken"` or
   `"dismissed"`), the Odoo shell, or an `ir.actions.server` code action -- can invoke this method
   on a record in any state, including a terminal one.

**Concrete scenario**: Moderator A opens a `"new"` report and clicks "Take Action & Strike User"
(`action_take_action_and_strike`): the owner is struck, possibly suspended, and `state` becomes
`"action_taken"`. Moderator B, viewing a browser tab opened before A's action (still rendering the
record as `"new"`, so B's "Reviewing" button is visible), clicks it. `action_mark_under_review()`
executes with no error, reverting `state` to `"under_review"` on an already-actioned report -- the
strike stays applied, but the report now looks like an open case awaiting review again, inviting a
second look (or a second, duplicate strike via `action_take_action_and_strike`, which itself has no
guard against re-firing on an already-`"action_taken"` report either, though that method is outside
this claim's own scope).

**Why not FOR-EACH**: this method is `self.write(...)`, a single ORM call across the whole
recordset, not a Python-level loop with per-item business logic. The FOR-EACH extension's
ISOLATION clause exists to ask "can processing one item affect another item's outcome" -- a real
question for a Python loop with per-iteration side effects (the cron in
`claims/cron_notify_pending_reports.md` is the motivating case). Here there is no such question:
`UPDATE ... SET state = 'dismissed' WHERE id IN (...)` is uniform across every row by construction,
so an ISOLATION clause would only ever read "trivially maintained," adding a form without adding
information. The substantive finding -- that the transition is unconditional regardless of each
record's own prior state -- is fully captured by statement 2's plain IF/THEN above. See this
skill's own Friction log for a proposed write-up of this as a recurring ambiguity.

**Verdict**: No state-machine transition guard exists anywhere in the call chain for this method --
not in the method body, not in the field definition, not in a model constraint, and not at the
access-control layer (which restricts *who* can call it, not *when*). The form view's `invisible`
condition is the only place this codebase expresses "this action only makes sense from `new`," and
it is UI-only. This is a real, confirmed gap: any administrator (or the service account, which also
has write access though no current code path uses it for this method) can move any report into
`"under_review"` from any state, including reopening an already-`"action_taken"` or
`"dismissed"` report with no audit trail explaining why. Severity is moderate (no data corruption,
no privilege escalation, no crash -- but a real audit/workflow-integrity defect enabling silent,
un-flagged state regression, most concretely reachable via the concurrent-moderator race above).
