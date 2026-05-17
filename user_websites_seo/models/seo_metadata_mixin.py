# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
from odoo import models, _
from odoo.exceptions import AccessError

class SEOMetadataMixin(models.AbstractModel):
    _name = "user.websites.seo.metadata.mixin"
    _description = "User Websites SEO Metadata Mixin"

    def _get_seo_fields(self):
        return {
            "website_meta_title",
            "website_meta_description",
            "website_meta_keywords",
            "website_meta_og_img",
            "seo_name"
        }

    def _check_seo_write_permission(self):
        """
        To be overridden by models using this mixin to define
        who can edit SEO metadata.
        """
        raise NotImplementedError("Each model must implement its own permission check.")

    def write(self, vals):
        seo_fields = self._get_seo_fields()
        seo_vals = {k: v for k, v in vals.items() if k in seo_fields}
        other_vals = {k: v for k, v in vals.items() if k not in seo_fields}

        res = True
        if other_vals:
            # Let standard Odoo ACLs handle non-SEO writes natively
            res = super(SEOMetadataMixin, self).write(other_vals)

        if seo_vals:
            if self.env.su or self.env.user.has_group("user_websites.group_user_websites_administrator"):
                res = res and super(SEOMetadataMixin, self).write(seo_vals)
            else:
                self._check_seo_write_permission()
                # Escalate strictly for the write operation using the domain service account
                # ADR-0001: Use with_context(mail_notrack=True)
                # LLM_EXPERIENCE: NEVER use prefetch_fields=False during .create() or on models without chatter.
                svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                    "user_websites.user_user_websites_service_account"
                )
                res = res and super(
                    SEOMetadataMixin,
                    self.with_user(svc_uid).with_context(mail_notrack=True)
                ).write(seo_vals)

        return res
