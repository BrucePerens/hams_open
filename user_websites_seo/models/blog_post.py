# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).
from odoo import models


# [@ANCHOR: user_websites_seo_blog_post_blog_post_seo]
class BlogPostSEO(models.Model):
    _name = "blog.post"
    _inherit = ["blog.post", "user.websites.seo.metadata.mixin"]

    # [@ANCHOR: user_websites_seo:COMM_post_check_seo_write_permission]
    def _check_seo_write_permission(self):
        self.check_access("write")
