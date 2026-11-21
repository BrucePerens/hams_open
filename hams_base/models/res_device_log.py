# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. License: AGPL-3.0.
from odoo import models


# [@ANCHOR: hams_base_res_device_log_res_device_log]
class ResDeviceLog(models.Model):
    _inherit = "res.device.log"

    # [@ANCHOR: hams_base:device_log_gc_off]
    # Verified by [@ANCHOR: hams_base:test_device_log_gc_off]
    def _gc_device_log(self):
        """Core's autovacuum deleted all but the latest device log of a session. Device logs are log records and log
        records are permanent (Bruce, 2026-11-20). Not decorated with `@api.autovacuum`, so the vacuum no longer runs it."""
        return None
