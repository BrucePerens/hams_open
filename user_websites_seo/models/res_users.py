# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
from odoo import models, _
from odoo.exceptions import AccessError


class ResUsersSEO(models.Model):
    _name = "res.users"
    _inherit = ["res.users", "user.websites.seo.metadata.mixin"]

    @property
    def SELF_WRITEABLE_FIELDS(self):
        # [@ANCHOR: res_users_self_writeable_fields]
        # Verified by [@ANCHOR: test_self_writeable_fields]
        return super().SELF_WRITEABLE_FIELDS + list(self._get_seo_fields())

    def _check_seo_write_permission(self):
        if not all(record.id == self.env.user.id for record in self):
            # [@ANCHOR: res_users_seo_write_elevation]
            # Verified by [@ANCHOR: test_seo_widget_tour]
            # Verified by [@ANCHOR: test_check_access_rule_res_users]
            raise AccessError(_("You can only modify your own SEO metadata."))
