# -*- coding: utf-8 -*-
from odoo import models

class ZeroSudoSecurityUtils(models.AbstractModel):
    _inherit = "zero_sudo.security.utils"

    def _get_param_read_whitelist(self):
        res = super()._get_param_read_whitelist()
        res.append("pager_duty.config_dir")
        return res
