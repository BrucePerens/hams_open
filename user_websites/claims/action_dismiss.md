---
anchor: user_websites:report_action_dismiss
code_hash: sha256:91bde8e637288fc68eb32f42b3c892be900f2f79f69b14238223bc6b52b44ff4
---

# Claim: `ContentViolationReport.action_dismiss`

Written after an adversarial bug-hunt pass (`hams_shared/agents/skills/bug-hunt/SKILL.md`)
reviewing this method together with the sibling `action_mark_under_review`
(`claims/action_mark_under_review.md`) -- both trivial one-line state-setters raising the identical
question: is any state-machine transition actually enforced, anywhere, or does `write()` accept any
value from any prior state? Phrased in EARS. Not phrased with the FOR-EACH extension -- see
`claims/action_mark_under_review.md`'s "Why not FOR-EACH" note, which applies identically here
(this method is also a single `self.write(...)` ORM call across the recordset, not a per-item
Python loop).

1. WHEN `action_dismiss()` is called on recordset `self` (one or more
   `content.violation.report` records), THE system SHALL set `state = "dismissed"` for every
   record in `self`, via the single ORM call `self.write({"state": "dismissed"})`.
2. IF any record in `self` has `state == "action_taken"` at the time of the call -- meaning a
   strike has already been permanently applied to its `content_owner_id` or `content_group_id` via
   the `increment_strike_count()` stored procedure, and `owner.action_suspend_user_websites()` /
   `group.action_suspend_group_websites()` may have already fired -- THEN THE system SHALL still
   overwrite that record's `state` to `"dismissed"`, with no check of the record's own prior state.
3. THE system SHALL NOT reverse, adjust, or even re-examine `violation_strike_count` or any
   suspension flag when statement 2's condition is true. The only code path that resets a strike
   count or lifts a suspension is `action_pardon_user_websites` (`res_users_moderation.py`, its own
   separate anchor `user_websites:COMM_action_pardon_user_websites`) and the equivalent group-level
   pardon action -- neither is called from, or in any way linked to, `action_dismiss`. Verified by
   grepping the whole module for `decrement`/`reverse_strike`/`undo_strike`: no such mechanism
   exists at all; strike counts only ever increment (`sql_views.py`) or reset via the pardon actions
   (`res_users_moderation.py:84`, `user_websites_groups.py:409`), never as a side effect of a
   report's own state changing.
4. THE system SHALL NOT raise, log, or message-post any indication that a `"action_taken"` ->
   `"dismissed"` transition is unusual or worth flagging -- the model has no `@api.constrains`, no
   `models.Constraint`, and no `write()` override restricting valid `state` transitions in either
   direction (verified by reading the complete model file; the only constraints defined are
   `_report_uniq`, `_url_not_empty`, `_desc_not_empty`, none touching `state`).
5. IF the calling user lacks `write` access on `content.violation.report` (`base.group_portal` and
   `base.group_public` are both `perm_write=0` in `security/ir.model.access.csv`), THEN THE system
   SHALL raise `odoo.exceptions.AccessError`, matching statement 4 of the
   `action_mark_under_review` claim -- the same *who may call* vs. *may this transition happen*
   distinction applies here, and it is the only enforcement boundary this method has.
6. THE system SHALL NOT be gated, beyond client-side widget rendering, by the form view's
   `invisible="not id or state in ('action_taken', 'dismissed')"` condition
   (`views/content_violation_report_views.xml:77`). That condition only suppresses the "Dismiss
   Report" button once a record is *already* `"action_taken"` or `"dismissed"` in the currently
   rendered form; it cannot prevent a caller from invoking `action_dismiss()` in the window between
   another moderator's `action_take_action_and_strike()` write committing and this moderator's own
   already-open form or direct RPC call executing against the same record id.

**Concrete two-moderator race**: Moderator A and Moderator B both open the same `"new"` report
(e.g., from the kanban queue). A clicks "Take Action & Strike User": the report's owner is struck
(possibly suspended) and `state` becomes `"action_taken"`, all before B acts. B's tab still shows
the report as it looked when first loaded (or B calls the method directly via RPC, bypassing the
UI's now-stale `invisible` evaluation entirely) and clicks "Dismiss Report". `action_dismiss()`
executes successfully with no error, overwriting `state` to `"dismissed"`. The report now reads as
a dismissed, no-violation-found case, while the reported user or group still carries the strike --
and possibly a live suspension -- applied moments earlier, with nothing in the record, the chatter,
or any exception connecting the two facts for a future auditor.

**Verdict**: Same structural gap as `action_mark_under_review`, and arguably more consequential
here because the two states it can silently override (`"action_taken"`) carry real, irreversible
side effects already applied elsewhere in the system (a strike, a suspension) that this method
neither checks for nor reconciles. No guard exists in the method body, the field definition, any
model constraint, or the access-control layer (which restricts *who*, not *when*). The form view's
`invisible` condition is the only place "don't dismiss an already-actioned report" is expressed,
and it is UI-only, defeated by any RPC caller or by the ordinary two-tab/two-moderator race
described above. Confirmed by reading; not run. Severity: moderate-to-real audit-integrity defect
(misleading final record state versus actual enforcement history), not a privilege escalation or
crash, but concretely reachable by two legitimately-privileged administrators working the same
queue concurrently -- not a contrived edge case.
