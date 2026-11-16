# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Version 1.1: the DBA bloat cron's `group_ids` reaches a database that already has the job.

`cron_check_bloat` was first loaded inside a noupdate="1" block and has been in a noupdate="0" block since 2026-09-28 (the
service account may not write the model, so Odoo 19 refuses the server action unless the job names the service's group). Odoo
checks the `noupdate` flag of the EXISTING `ir_model_data` row as well as the block, and moving the record never cleared it: on
a database that had the module before then the group never reached the row and the job was refused every day. This clears the
flag before the same upgrade loads the data file, which then writes the group.

Plain SQL, like every migration here. Running it again, or on a fresh install (no version), changes nothing."""


def migrate(cr, version):
    if not version:
        return
    cr.execute(  # audit-ignore-sql: static SQL, literal xml id
        "UPDATE ir_model_data SET noupdate = FALSE WHERE module = 'database_management' "
        "AND name = 'cron_check_bloat' AND noupdate"
    )
