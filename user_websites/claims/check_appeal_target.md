---
anchor: user_websites:COMM_check_appeal_target
code_hash: sha256:fabca92a27fbdafde65699fa50ac5a25abc1a571e72ed189c417e830cb9f6c2c
---

# Claim: `ContentViolationAppeal._check_appeal_target`

Written after an adversarial bug-hunt pass (`hams_shared/agents/skills/bug-hunt/SKILL.md`) that
independently re-derived, from the actual vendored Odoo ORM source (`odoo` 19.0,
`/usr/lib/python3/dist-packages/odoo/orm/{models,fields}.py`), whether the field-level comment on
`user_id`/`group_id` ("an explicit default (even False) guarantees user_id/group_id are always in
vals, so `_check_appeal_target()` reliably fires...") is actually true, rather than trusting the
comment's own account of the ORM's behavior. Phrased in EARS, extended with the FOR-EACH pattern
for the per-record loop this method itself contains.

FOR-EACH `appeal` IN `self` (every record the ORM passes to this `@api.constrains("user_id",
"group_id")` method for one create()/write() call -- see statements 4-6 below for exactly when
that happens):

1. WHEN `appeal.user_id` and `appeal.group_id` are both falsy, OR both truthy, THE system SHALL
   raise `odoo.exceptions.ValidationError` ("An appeal must be tied to either a User or a Group,
   but not both.").
2. IF exactly one of `appeal.user_id`/`appeal.group_id` is truthy, THEN THE system SHALL NOT raise,
   for this appeal.
3. IF an earlier appeal in iteration order within the same call already triggered statement 1,
   THEN THE system SHALL NOT evaluate statements 1-2 for any appeal later in iteration order within
   that same call -- the `raise` inside `for appeal in self:` stops the loop at the first violation
   found; later appeals in the same batch are never checked in that pass.

**ISOLATION: NOT maintained across appeals in the same create()/write() call.** One appeal's
invalid target state aborts the whole call: the raised `ValidationError` propagates out of
`create()`/`write()`, whose own field writes for every record in the batch already executed
against the in-memory/DB layer before this method ran (`_validate_fields` is called *after* the
column writes), so the exception unwinds the surrounding transaction and prevents ALL appeals in
that batch from persisting, not just the invalid one -- a real cross-item effect, though it is
ordinary Odoo all-or-nothing batch-write semantics rather than a defect specific to this function.
Noted as friction against the FOR-EACH pattern in the accompanying bug-hunt report: this ISOLATION
question is actually a property of the ORM's batch-write atomicity, not something this function
body controls one way or the other.

Ubiquitous statements on when this method is invoked at all (the actual create()/write() trigger
mechanism, verified against `odoo/orm/models.py`'s `create()`/`_create()`/`write()`/`default_get`/
`_add_missing_default_values` and `odoo/orm/fields.py`'s `Field._setup_attrs__`, not assumed from
the field-level comment):

4. WHEN a `create()` call's fully-resolved, post-default stored values (`data['stored']` in
   `Model._create()`) include the key `user_id` or `group_id` for one or more records being
   created, THE system SHALL invoke this method against every record created in that same
   `create()` call (`Model._validate_fields` is called once per `create()` with the union of all
   records' stored field names).
5. WHEN a `write()` call's caller-supplied `vals` dict -- unmodified by any field-level default;
   `write()` never calls `default_get`/`_add_missing_default_values` -- contains the key `user_id`
   or `group_id`, THE system SHALL invoke this method against every record in that `write()` call's
   recordset, evaluated once, after all of that call's own field writes have already been applied
   (so a single `write()` call that sets both fields at once is checked against its own final
   post-write state, not a partial mid-write state).
6. IF a `write()` call's `vals` contains neither `user_id` nor `group_id`, THEN THE system SHALL
   NOT invoke this method for that call, even if one or more affected records' current,
   already-persisted state already violates statements 1-2 -- e.g. `action_approve()`/
   `action_reject()`'s own state-only writes (`appeal.state = "approved"`/`"rejected"`) never
   re-check this invariant on an already-invalid row, because neither watched field is a key in
   those calls' vals.
7. Because `user_id` and `group_id` are declared with `default=False` rather than no default at
   all, THE system SHALL include both keys, each valued `False`, in a `create()` call's final
   stored values whenever the caller's own vals passed to `create()` omit both keys -- confirmed by
   reading, in order: `Field._setup_attrs__` (`orm/fields.py` ~513-516) wraps *any* non-`None`
   default value, including the literal `False`, into a callable (`self.default = lambda model:
   value`); `Model.default_get` (`orm/models.py` ~1304) gates inclusion on `if field.default:` --
   true for that wrapped callable regardless of what it *returns* -- not on the truthiness of the
   value it will produce; `default_get`'s own conversion loop (~1328-1345) does not drop a falsy
   converted value from `defaults`; and `Model._create()`'s classification loop (~4677-4679) sets
   `stored[key] = val` unconditionally whenever `field.store` is true, never conditioned on `val`'s
   truthiness. So statement 4's own trigger condition is reliably met even for a `create()` call
   supplying neither field -- exactly the scenario the field-level comment names, and exactly the
   scenario `test_appeals_and_views.py`'s `create({"reason": "No target"})` exercises.
8. IF `user_id`/`group_id` instead had no default at all (Odoo's ordinary case for a field a caller
   doesn't supply), THEN a `create()` call omitting both fields would leave neither key in
   `data['stored']` (since `default_get` would return no entry for either name, per the `if
   field.default:` gate in statement 7 being false for an unset default), and THE system would NOT
   invoke this method for that record -- allowing an appeal tied to neither a user nor a group to
   be created successfully. This is the counterfactual the field-level comment's own reasoning
   rests on; verified true by reading the same code path as statement 7, not merely inferred from
   the comment's own assertion.

**Precision note on the field-level comment (not a functional bug):** the comment states the
mechanism as "the create()/write() vals" as if the default's guarantee applied to both call types
equally. It does not: `write()` never applies `field.default` to fill in omitted keys (statement
5) -- only `create()` does (statements 4, 7). This has no live behavioral consequence for this
specific model, because `user_id`/`group_id` are plain stored `Many2one` fields with no `inverse`/
`related`/`compute` -- the *only* way either field's boolean state can change via `write()` is by
that field's own name being a literal key in that specific `write()` call's vals, which already
guarantees statement 5's trigger regardless of any default. But the comment's phrasing overclaims
the default's role for `write()`, and would become actively misleading if a future change gave
either field an `inverse` or `related` mechanism capable of changing it without the field's own
name appearing as a `write()` vals key.

**Sole enforcement, and class 4 (latent, no live bypass found):** this `@api.constrains` method is
the *only* enforcement of "exactly one of user_id/group_id" anywhere in this codebase -- no
`models.Constraint`/`_sql_constraints`/DB `CHECK` backs it (grepped the model file and the whole
repo for both). This matches `hams_shared/agents/skills/bug-hunt/SKILL.md`'s own bug-class-4
candidate-linter note almost exactly ("a grep for field pairs whose names/docstrings use
exclusivity language... without a matching `models.Constraint`/`_sql_constraints` nearby") -- the
raised message's own text ("tied to either a User or a Group, but not both") is exactly that
exclusivity language. Unlike bug class 4's original found instance, nothing here *claims* DB-level
enforcement exists, so this is a latent defense-in-depth gap, not a doc/code mismatch. Grepped the
whole `hams_open` tree for any bypass of the ORM path this constraint relies on: no raw
`cr.execute`/SQL writer touching `content_violation_appeal`, no direct `._write(`/`._write_multi(`
call on this model, no XML `<record>` data file creating appeal rows, and no `create()`/`write()`
override on `ContentViolationAppeal` itself. So no live bypass path exists in this codebase today;
a future raw-SQL migration, bulk import/fixture, or a `_write`/`_write_multi` call added later
would not be caught by this method. `Model._validate_fields` also runs this check against
`self.sudo()` regardless of the calling user's own rights (`records = self.sudo()` in
`_validate_fields`), so `sudo()` on the *caller* side neither bypasses nor weakens this constraint
-- it is not an access-control check, it always evaluates once its trigger condition (statements
4-6) is met.
