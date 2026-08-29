# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
from odoo import models


class Website(models.Model):
    _inherit = "website"

    def _search_get_details(self, search_type, order, options):
        result = super()._search_get_details(search_type, order, options)
        if search_type in ("knowledge_articles", "all"):
            result.append(
                self.env["knowledge.article"]._search_get_detail(self, order, options)
            )
        return result
