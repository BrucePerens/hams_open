# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
from odoo import models


class BlogPostSEO(models.Model):  # burn-ignore-env
    _name = "blog.post"
    _inherit = ["blog.post", "user.websites.seo.metadata.mixin"]

    def _check_seo_write_permission(self):
        self.check_access("write")
