# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
from odoo import models, _
from odoo.exceptions import AccessError

class ResUsersSEO(models.Model):
    _name = "res.users"
    _inherit = ["res.users", "website.seo.metadata"]

    @property
    def SELF_WRITEABLE_FIELDS(self):
        # [@ANCHOR: res_users_self_writeable_fields]
        # Verified by [@ANCHOR: test_self_writeable_fields]
        return super().SELF_WRITEABLE_FIELDS + [
            "website_meta_title",
            "website_meta_description",
            "website_meta_keywords",
            "website_meta_og_img",
            "seo_name",
        ]

    def write(self, vals):
        seo_fields = {"website_meta_title", "website_meta_description", "website_meta_keywords", "website_meta_og_img", "seo_name"}
        seo_vals = {k: v for k, v in vals.items() if k in seo_fields}
        other_vals = {k: v for k, v in vals.items() if k not in seo_fields}

        res = True
        if other_vals:
            # Let standard Odoo ACLs handle non-SEO writes natively
            res = super(ResUsersSEO, self).write(other_vals)

        if seo_vals:
            if self.env.su or self.env.user.has_group("user_websites.group_user_websites_administrator"):
                res = res and super(ResUsersSEO, self).write(seo_vals)
            else:
                if all(record.id == self.env.user.id for record in self):
                    # [@ANCHOR: res_users_seo_write_elevation]
                    # Verified by [@ANCHOR: test_seo_widget_tour]
                    # Verified by [@ANCHOR: test_check_access_rule_res_users]
                    # Escalate strictly for the write operation using the domain service account
                    # ADR-0001: Use with_context(mail_notrack=True, prefetch_fields=False)
                    svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid("user_websites.user_user_websites_service_account")
                    res = res and super(ResUsersSEO, self.with_user(svc_uid).with_context(mail_notrack=True, prefetch_fields=False)).write(seo_vals)
                else:
                    raise AccessError(_("You can only modify your own SEO metadata."))

        return res
