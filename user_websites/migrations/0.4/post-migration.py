# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Remove the orphaned `content.violation.report` ir.rule and ir.sequence
records this module stopped declaring when commit 2097adbc (2026-09-23)
extracted `content.violation.report` into the `content_moderation` module.
[@ANCHOR: user_websites_orphan_content_violation_cleanup]

Odoo never deletes a record a module stops declaring when that record's
`ir_model_data` row is `noupdate=True`: `ir.model.data._process_end()`
only purges stale *updatable* xmlids. These five were all declared inside
`noupdate="1"` blocks, so every database that had `user_websites`
installed before the extraction (hams_prod included, verified read-only
2026-10-02) still carries them, owned by `user_websites`, next to
`content_moderation`'s own replacements:

* `content_violation_report_{user,multi_company,admin,service_account}_rule`
  -- redundant duplicates of content_moderation's four rules (identical
  domains and permissions). The two that were scoped to this module's own
  groups (admin -> group_user_websites_administrator, service_account ->
  group_user_websites_service_account) are covered by those groups'
  `implied_ids` links into content_moderation's moderator/service-account
  groups (user_websites_security.xml's own noupdate="0" block), and Odoo
  19 evaluates rules against `res.users.all_group_ids` (implied closure),
  so removing them changes no one's effective access.
* `seq_content_violation_report` -- a second `ir.sequence` with code
  `content.violation.report`. `ir.sequence.next_by_code()` orders only by
  `company_id`, and both rows have `company_id` NULL, so which one it
  would draw from is undefined. Moot in practice today (no caller of
  `next_by_code("content.violation.report")` exists in any repository and
  neither PostgreSQL sequence had ever been drawn from on hams_prod), but
  leaving two would make any future caller's numbering nondeterministic.

Plain SQL, matching every other migration script in this codebase: the
zero-sudo rule forbids building a superuser environment here. Only the
exact five xmlids under module `user_websites` are touched, and each is
additionally constrained to the expected model (ir.rule on
content.violation.report / ir.sequence with code content.violation.report),
so anything else that happens to share a name is left alone. A database
that never had them (a fresh install) is a no-op. Sequence removal
mirrors `ir.sequence.unlink()` / `ir.sequence.date_range.unlink()`: the
backing PostgreSQL sequences are dropped with Odoo's own
`_drop_sequences()` before the rows are deleted."""
import logging

from odoo.addons.base.models.ir_sequence import _drop_sequences


_logger = logging.getLogger(__name__)

ORPHAN_RULE_XMLIDS = (
    "content_violation_report_user_rule",
    "content_violation_report_multi_company_rule",
    "content_violation_report_admin_rule",
    "content_violation_report_service_account_rule",
)
ORPHAN_SEQUENCE_XMLID = "seq_content_violation_report"
REPORT_MODEL = "content.violation.report"


def _remove_orphan_rules(cr):
    cr.execute(  # audit-ignore-sql: static SQL, parameters are this migration's own hardcoded xmlids/model name
        """
        SELECT d.id, r.id
          FROM ir_model_data d
          JOIN ir_rule r ON r.id = d.res_id
          JOIN ir_model m ON m.id = r.model_id
         WHERE d.module = 'user_websites'
           AND d.model = 'ir.rule'
           AND d.name = ANY(%s)
           AND m.model = %s
        """,
        (list(ORPHAN_RULE_XMLIDS), REPORT_MODEL),
    )
    rows = cr.fetchall()
    if not rows:
        return
    data_ids = [row[0] for row in rows]
    rule_ids = [row[1] for row in rows]
    # rule_group_rel rows go with ON DELETE CASCADE.
    cr.execute(  # audit-ignore-sql: ids resolved by the query above, not user input
        "DELETE FROM ir_rule WHERE id = ANY(%s)", (rule_ids,)
    )
    cr.execute(  # audit-ignore-sql: ids resolved by the query above, not user input
        "DELETE FROM ir_model_data WHERE id = ANY(%s)", (data_ids,)
    )
    _logger.info(
        "user_websites: removed %d orphaned content.violation.report "
        "ir.rule records %s",
        len(rule_ids),
        rule_ids,
    )


def _remove_orphan_sequence(cr):
    cr.execute(  # audit-ignore-sql: static SQL, parameters are this migration's own hardcoded xmlid/code
        """
        SELECT d.id, s.id
          FROM ir_model_data d
          JOIN ir_sequence s ON s.id = d.res_id
         WHERE d.module = 'user_websites'
           AND d.model = 'ir.sequence'
           AND d.name = %s
           AND s.code = %s
        """,
        (ORPHAN_SEQUENCE_XMLID, REPORT_MODEL),
    )
    row = cr.fetchone()
    if not row:
        return
    data_id, seq_id = row
    cr.execute(  # audit-ignore-sql: seq_id resolved by the query above, not user input
        "SELECT id FROM ir_sequence_date_range WHERE sequence_id = %s",
        (seq_id,),
    )
    range_ids = [range_row[0] for range_row in cr.fetchall()]
    pg_names = ["ir_sequence_%03d" % seq_id]
    pg_names += [
        "ir_sequence_%03d_%03d" % (seq_id, range_id)
        for range_id in range_ids
    ]
    _drop_sequences(cr, pg_names)
    # ir_sequence_date_range rows go with ON DELETE CASCADE.
    cr.execute(  # audit-ignore-sql: seq_id resolved by the query above, not user input
        "DELETE FROM ir_sequence WHERE id = %s", (seq_id,)
    )
    cr.execute(  # audit-ignore-sql: data_id resolved by the query above, not user input
        "DELETE FROM ir_model_data WHERE id = %s", (data_id,)
    )
    _logger.info(
        "user_websites: removed orphaned content.violation.report "
        "ir.sequence %d",
        seq_id,
    )


def migrate(cr, version):
    if not version:
        return
    _remove_orphan_rules(cr)
    _remove_orphan_sequence(cr)
