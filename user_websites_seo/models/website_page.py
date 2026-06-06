# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
from odoo import models

class WebsitePageSEO(models.Model):
    _name = "website.page"
    _inherit = ["website.page", "user.websites.seo.metadata.mixin"]

    def _check_seo_write_permission(self):
        # reuse the logic from user_websites
        # we need to be careful not to introduce N+1
        self.check_access("write")
        # if check_access passes, it means the user is either admin or owner/member
        # of the page. This is enough for SEO.
