# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
from odoo import models, _
from odoo.exceptions import AccessError


class UserWebsitesGroupSEO(models.Model):
    _name = "user.websites.group"
    _inherit = [
        "user.websites.group",
        "website.seo.metadata",
        "user.websites.seo.metadata.mixin"
    ]

    def _check_seo_write_permission(self):
        if not all(self.env.user.id in group.member_ids.ids for group in self):
            # [@ANCHOR: user_websites_group_seo_write_elevation]
            # Verified by [@ANCHOR: test_seo_widget_tour]
            # Verified by [@ANCHOR: test_check_access_rule_user_websites_group]
            raise AccessError(_(
                "You can only modify SEO metadata for groups "
                "you are a member of."
            ))
