# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).
from odoo import models


# [@ANCHOR: user_websites_seo_website_page_website_page_seo]
class WebsitePageSEO(models.Model):
    _name = "website.page"
    _inherit = ["website.page", "user.websites.seo.metadata.mixin"]

    # [@ANCHOR: user_websites_seo:COMM_page_check_seo_write_permission]
    def _check_seo_write_permission(self):
        self.check_access("write")
