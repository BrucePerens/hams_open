# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Pager Duty 1.1: the "never run" check status, a grace period for checks that never ran, and the board's new count.

[@ANCHOR: pager_duty:never_run_migration]

* `pager.check.status` gains "never_run" and it becomes the default. A check that no monitor has ever produced a result for
  (`last_run` is empty) used to be stored as "passing"; on hams1 all 24 rows were, while no monitor daemon had ever been
  installed. Those rows become "never_run". A row with a `last_run` has really been run by a monitor, so its status is kept.
* A check that has never run and has no startup grace period gets 300 seconds, so the first runs of a freshly started monitor
  (before every dependent service and credential is known good) cannot report at once. A row that has run keeps whatever grace
  period an administrator chose.
* The board's stored procedure is installed only by `post_init_hook`, which a module upgrade does not run, so it is installed
  again here: it now also counts "never_run".

Plain SQL, matching this platform's zero-sudo rule for migration scripts. Running it again changes nothing further."""

import logging

_logger = logging.getLogger(__name__)


def migrate(cr, version):
    cr.execute(
        "UPDATE pager_check SET status = 'never_run' WHERE last_run IS NULL AND status = 'passing'"
    )
    _logger.info("pager_duty 1.1: %s check(s) that never ran are now 'never_run'.", cr.rowcount)
    cr.execute(
        "UPDATE pager_check SET grace_period = 300 WHERE last_run IS NULL AND COALESCE(grace_period, 0) = 0"
    )
    _logger.info("pager_duty 1.1: %s never-run check(s) given a 300 second startup grace period.", cr.rowcount)

    from odoo.addons.pager_duty.hooks import _PAGER_BOARD_SQL

    cr.execute(_PAGER_BOARD_SQL)
