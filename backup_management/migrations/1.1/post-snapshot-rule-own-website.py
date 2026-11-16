# Copyright © HAMS project. AGPL-3.0-or-later.
"""backup_management 1.1: backup snapshots survive their configuration being deleted (config_id is set null), so the
snapshot tenant rule reads the snapshot's own stored website_id instead of config_id.website_id (False for an orphan, which
would show it to every website's admin). The rule is in a noupdate block, so an existing database needs this write.

[@ANCHOR: backup_management:migration_snapshot_rule_own_website]
Verified by [@ANCHOR: backup_management:test_migration_snapshot_rule_own_website]
Raw SQL on the cursor, idempotent."""

_OLD = "('config_id.website_id', '=', False), ('config_id.website_id', '=', user.website_id.id)"
_NEW = "('website_id', '=', False), ('website_id', '=', user.website_id.id)"


def migrate(cr, version):
    if not version:
        return
    cr.execute(  # audit-ignore-sql: static SQL, values passed as parameters
        """UPDATE ir_rule SET domain_force = REPLACE(domain_force, %s, %s)
            WHERE id = (SELECT res_id FROM ir_model_data
                         WHERE module = 'backup_management' AND name = 'rule_backup_snapshot_multi_tenant' AND model = 'ir.rule')""",
        (_OLD, _NEW),
    )
