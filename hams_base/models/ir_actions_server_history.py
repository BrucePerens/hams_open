# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. License: AGPL-3.0.
from odoo import models


# [@ANCHOR: hams_base_ir_actions_server_history_ir_actions_server_history]
class IrActionsServerHistory(models.Model):
    _inherit = "ir.actions.server.history"

    # [@ANCHOR: hams_base:server_action_history_gc_off]
    # Verified by [@ANCHOR: hams_base:test_server_action_history_gc_off]
    def _gc_histories(self):
        """Core's autovacuum deleted the oldest code revisions of a server action beyond `_max_entries_per_action`.
        Stored information is never purged (Bruce, 2026-11-20). Not decorated with `@api.autovacuum`, so the vacuum
        no longer runs it."""
        return None
