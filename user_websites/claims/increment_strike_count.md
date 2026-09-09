---
anchor: user_websites:COMM_increment_strike_count
code_hash: sha256:71115267d13000b34691b1d0edd2f5be250d719ae1077fe50a806b7c641ee284
---

# Claim: `ContentViolationReport._increment_strike_count`

Written after an adversarial bug-hunt pass (`hams_shared/agents/skills/bug-hunt/SKILL.md`) that
checked this method's own docstring claim -- "Atomically locks and increments a strike count via
the increment_strike_count() stored procedure (sql_views.py)" -- against the real PL/pgSQL body of
that stored procedure (`UserWebsitesDbFunctions.init()`,
`user_websites/models/sql_views.py:150-174`), rather than trusting the docstring's own account of
it. Phrased in EARS, extended with the FOR-EACH pattern for the concurrency question the
docstring's own word "Atomically" raises.

1. WHEN this method is called with `table_name` equal to the literal string `"res_users"` and
   `rec_id` equal to the primary key of an existing row of that table, THE system SHALL acquire a
   `FOR NO KEY UPDATE` row lock on that row and then increment that row's own
   `violation_strike_count` column by exactly 1, within the calling transaction (`self.env.cr`'s
   own transaction; the stored procedure opens no sub-transaction of its own).
2. WHEN this method is called with `table_name` equal to the literal string
   `"user_websites_group"` and `rec_id` equal to the primary key of an existing row of that table,
   THE system SHALL acquire a `FOR NO KEY UPDATE` row lock on that row and then increment that
   row's own `violation_strike_count` column by exactly 1, by the same mechanism as statement 1.
3. THE system SHALL NOT interpret `table_name` as a dynamic SQL identifier (no `EXECUTE`/`format()`
   construction anywhere inside the stored procedure): the procedure only ever compares
   `table_name`'s value against the two string literals above (`IF tbl_name = 'res_users' ...
   ELSIF tbl_name = 'user_websites_group'`), so a `table_name` value from any source cannot be
   used to target, or inject SQL against, a table outside this fixed pair.
4. IF `table_name` is any value other than the two literal strings named in statements 1 and 2
   (a typo, an unrelated table name, `None`, or an empty string), THEN THE system SHALL take no
   row lock, execute no `UPDATE`, raise no exception, log nothing, and return normally -- a silent
   no-op the caller has no way to distinguish from a successful increment of a row whose count
   happens not to have changed, since the underlying SQL function returns `void` either way.
5. IF `table_name` is one of the two literal strings named in statements 1 and 2 but `rec_id` does
   not match the primary key of any existing row of that table, THEN THE system SHALL take no row
   lock (the `PERFORM ... FOR NO KEY UPDATE` matches zero rows), execute an `UPDATE` that matches
   and changes zero rows, raise no exception, log nothing, and return normally -- the same kind of
   silent no-op as statement 4, reachable independently of it.
6. THE system SHALL NOT read or write any column other than `violation_strike_count`, and no row
   other than the single row identified by `table_name`/`rec_id`.
7. THE system SHALL NOT itself commit, flush, or invalidate any in-memory recordset cache; the
   caller (`action_take_action_and_strike`) is responsible for its own subsequent
   `invalidate_recordset(["violation_strike_count"])` call to make the new value visible to Odoo's
   ORM cache.
8. THE explicit `PERFORM id FROM ... FOR NO KEY UPDATE` statement changes nothing about the
   FOR-EACH clause's own outcome below: Odoo cursors run at PostgreSQL `REPEATABLE READ` isolation,
   not the PostgreSQL default `READ COMMITTED` (confirmed by reading the installed Odoo source
   directly -- `sql_db.py`: `self.connection.set_isolation_level(ISOLATION_LEVEL_REPEATABLE_READ)`
   -- not assumed). Under `REPEATABLE READ`, a bare `UPDATE ... WHERE id = rec_id` with no preceding
   lock statement would raise the identical error, on the identical conflict, that the FOR-EACH
   clause describes; the explicit pre-lock adds no serialization guarantee beyond what this
   isolation level already gives a plain `UPDATE`.

FOR-EACH `pair of invocations (A, B)` targeting the SAME `table_name`/`rec_id` from two different,
concurrently-running database transactions, where B's own transaction snapshot was already open
before A's transaction commits: WHEN B attempts to lock or update that row (immediately, if A has
already committed by the time B's own `PERFORM ... FOR NO KEY UPDATE` runs; or after first blocking
until A's transaction ends, if A is still open when B reaches that statement) and A's transaction
committed a change to that same row, THE system SHALL raise
`psycopg2.errors.SerializationFailure` out of B's call to this method -- uncaught by any
`try`/`except` inside `_increment_strike_count` or inside `action_take_action_and_strike` -- rather
than either (a) silently applying B's increment against a stale pre-A value (the lost-update the
docstring's "Atomically" is presumably meant to rule out), or (b) silently blocking-then-applying
B's increment in sequence against A's post-commit value (this claim's own first-draft description
of the mechanism, before checking Odoo's actual isolation level -- corrected here: PostgreSQL's
`REPEATABLE READ` does not silently re-read and retry a blocked writer the way `READ COMMITTED`
does; it aborts the second writer instead). IF A's transaction instead rolls back (rather than
committing) before B's attempt resolves, THEN THE system SHALL let B proceed and apply its own
increment normally, with no error and no interaction with A's now-discarded change. Whether B's
increment, in the commit case, is ultimately *lost* therefore depends entirely on machinery outside
this function and outside this claim's own anchor: both of this method's two real call paths
(`content_violation_report_views.xml`'s admin button and `website_page.py`'s hook, each reached as
an ordinary web/JSON-RPC request) execute under Odoo's own `odoo.http.Request` dispatch, which wraps
request serving in `odoo.service.model.retrying(serve_func, env=self.env)`
(`odoo/http.py`, confirmed by reading the installed source directly, not the `execute_kw`/XML-RPC
path this claim's own first draft cited instead) -- `retrying()` catches exactly this exception
class (`PG_CONCURRENCY_EXCEPTIONS_TO_RETRY` includes `errors.SerializationFailure`), rolls back, and
retries the whole request up to 5 times with exponential backoff. `_increment_strike_count` itself
SHALL NOT be relied upon to provide this retry; a hypothetical future caller outside that dispatch
path (a cron job, a raw script) would see this exception abort its own transaction instead of
transparently retrying.

**ISOLATION: THE system SHALL NOT alter, lock, or read any row other than the single row identified
by that invocation's own `table_name`/`rec_id` pair, for any other invocation running concurrently
against a different `table_name`/`rec_id`** -- verified by reading the PL/pgSQL body: both branches'
`PERFORM` and `UPDATE` statements carry a `WHERE id = rec_id` clause scoped to one table, with no
`UPDATE ... FROM`, subquery, or trigger that could reach a second row. Two concurrent invocations
against different rows (whether both `res_users`, both `user_websites_group`, or one of each) never
block or interfere with each other.

## Findings against the docstring's own claim

The docstring's word "Atomically" survives adversarial reading, but only in the precise sense the
FOR-EACH clause states: `_increment_strike_count` never *silently* loses an update against a
same-row concurrent writer -- it either applies the full, correct +1 or raises loudly
(`SerializationFailure`), with no third, silently-wrong outcome. It does not mean what the phrase
"atomically locks and increments" would naturally suggest to a reader (that concurrent callers
transparently queue and each succeed) -- that transparent success is a property of Odoo's own
`retrying()` dispatch wrapper sitting above this function's two real call paths, not of this
function or its stored procedure. Verified by reading the actual PL/pgSQL body, Odoo's own
`sql_db.py` isolation-level setting, and `service/model.py`'s retry logic directly -- not assumed
from the docstring, and not confirmed by any test. `test_05_concurrent_strike_locking()` in
`user_websites/tests/test_moderation.py`, despite its name and its own docstring claiming to
"verify that action_take_action_and_strike issues a FOR NO KEY UPDATE lock to prevent 'Lost Update'
race conditions during concurrent moderation," never actually runs two concurrent transactions
against the same row -- it wraps `_increment_strike_count` with
`wraps=ReportModel._increment_strike_count` and asserts only that it was *called* with the right
`table_name`/`rec_id` arguments for the user and group paths. That is a bug-class-3 instance
(claimed test/verification coverage that doesn't actually exist) on a sibling test, not on this
claim's own subject, but it means the atomicity guarantee above rests entirely on this pass's
reading of the SQL and the Odoo runtime source, not on any executable check. The sibling claim
`user_websites/claims/action_take_action_and_strike.md` (written by a parallel pass on the direct
caller) cites this same test as support for "safe against lost updates given the stored procedure's
own atomic per-row lock ... per `test_05_concurrent_strike_locking`'s own assertion" -- that
citation should be treated with the same skepticism this claim applies to it; the underlying
atomicity conclusion happens to still be correct (per the FOR-EACH clause above), but not for the
reason that sibling claim's own test citation implies.

Separately, statements 1-2's "increment ... by exactly 1" assumes the column's current value is a
real integer. `violation_strike_count` has no `NOT NULL`/`required=True` on either
`res.users.violation_strike_count` or `user.websites.group.violation_strike_count` (both plain
`fields.Integer(default=0)`, no `required=True`), so the column is nullable at the schema level; in
SQL, `NULL + 1` evaluates to `NULL`, meaning `UPDATE ... SET violation_strike_count =
violation_strike_count + 1` on a row whose stored value is SQL `NULL` would leave it `NULL`
(unchanged), raising no exception, while Odoo's ORM reads a `NULL` Integer field back as `0` -- so
the 3-strike threshold in `action_take_action_and_strike` would never trigger for that row no matter
how many strikes were "applied." This is an unenforced-invariant gap ("this column is always a real
integer, never SQL `NULL`") at the data layer, not a demonstrated bug, and its reachability through
the ORM is essentially nil: `Integer.convert_to_column` (`odoo/orm/fields_numeric.py`) is
`int(value or 0)`, so an ordinary ORM write of `None`/`False` on this field is coerced to `0` before
it ever reaches SQL, and `_init_column` backfills existing rows with the field's own default at
module-install time -- confirmed by reading the field implementation directly, not assumed. A
genuine SQL `NULL` in this column would require a path that bypasses the ORM's own column
conversion entirely (a raw `INSERT`/`UPDATE` via `cr.execute`, or a hand-written migration script),
not any write reachable through `res.users`/`user.websites.group`'s normal ORM interface. Flagged as
a data-layer invariant with no `CHECK`/`NOT NULL` enforcing it, not as a reachable, demonstrated bug.

What the docstring does not mention, and what an adversarial read of the stored procedure surfaces,
is statements 4 and 5 above: the PL/pgSQL `IF ... ELSIF ...` chain has no final `ELSE` branch, and
neither the Python wrapper nor the SQL function validates `rec_id` against the target table before
running the `UPDATE`. Both branches make this method fail silently rather than fail loudly on bad
input -- in tension with this codebase's own standing fail-fast principle (an error path should
never silently fall back to a no-op that can mask a real bug). This looks like a candidate new
bug-hunt class -- a generic dispatch/enum parameter with no default-raise branch, silently no-op-ing
on any unmatched value instead of raising -- proposed in this pass's own report rather than added
to `hams_shared/agents/skills/bug-hunt/SKILL.md` directly, since other bug-hunt passes are running
concurrently against this same file's sibling functions and that shared skill file tonight.

Today's only two call sites (`action_take_action_and_strike`: `"res_users"`/`owner.id` and
`"user_websites_group"`/`group.id`) both pass literal, correctly-spelled table names and record IDs
read from live, FK-constrained `Many2one` fields (`content_owner_id`, `content_group_id`) moments
earlier in the same transaction, so neither silent-no-op branch (statement 4 or 5) is reachable
through any currently-shipped code path -- this is a latent defense-in-depth gap, not a demonstrated
production bug, and is reported as such.

Both actual call sites' target tables were independently confirmed (not assumed from the stored
procedure's own naming, or from the docstring) to really carry the `violation_strike_count` column
the procedure increments: `res.users` gains it via `ResUsersModeration.violation_strike_count` (an
ordinary, stored `fields.Integer`, `user_websites/models/res_users_moderation.py`), and
`user.websites.group` declares its own `violation_strike_count` `fields.Integer` directly
(`user_websites/models/user_websites_groups.py`). The stored procedure is not silently assuming a
column exists on one of its two hard-coded tables that doesn't actually hold for the other.
