# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. License: AGPL-3.0.
from odoo import models


# [@ANCHOR: hams_base_res_users_log_res_users_log]
class ResUsersLog(models.Model):
    _inherit = "res.users.log"

    # [@ANCHOR: hams_base:user_log_gc_off]
    # Verified by [@ANCHOR: hams_base:test_user_log_gc_off]
    def _gc_user_logs(self):
        """Core's autovacuum deleted every login record but each user's latest. Login records are log records and log
        records are permanent (Bruce, 2026-11-20). Not decorated with `@api.autovacuum`, so the vacuum no longer runs it."""
        return None
