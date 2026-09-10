# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).
from odoo import models


class SEOMetadataMixin(models.AbstractModel):  # burn-ignore-env
    _name = "user.websites.seo.metadata.mixin"
    _description = "User Websites SEO Metadata Mixin"

    # [@ANCHOR: user_websites_seo:COMM_get_seo_fields]
    def _get_seo_fields(self):
        return {
            "website_meta_title",
            "website_meta_description",
            "website_meta_keywords",
            "website_meta_og_img",
            "seo_name",
        }

    # [@ANCHOR: user_websites_seo:COMM_mixin_check_seo_write_permission]
    def _check_seo_write_permission(self):
        """
        To be overridden by models using this mixin to define
        who can edit SEO metadata.
        """
        raise NotImplementedError("Each model must implement its own permission check.")

    # [@ANCHOR: user_websites_seo:COMM_mixin_write]
    def write(self, vals):
        if self.env.context.get("skip_seo_metadata_mixin"):
            return super().write(vals)

        if self.env.su or self.env.user.has_group("user_websites.group_user_websites_administrator"):
            return super().write(vals)

        seo_fields = self._get_seo_fields()
        seo_vals = {k: v for k, v in vals.items() if k in seo_fields}
        other_vals = {k: v for k, v in vals.items() if k not in seo_fields}

        # bug-hunt (2026-09-09): permission for `seo_vals` must be checked
        # BEFORE `other_vals` is written, not after. This mixin's own
        # `_check_seo_write_permission()` override can be STRICTER than the
        # model's normal write ACL (res.users: self-only; user.websites.group:
        # member-only) -- so a caller with genuine write access to the
        # record (e.g. a real Odoo admin not in the
        # user_websites_administrator convenience-bypass group above) but
        # who fails the mixin's own extra check could previously get a
        # single write(vals) call that partially applies: `other_vals`
        # already persisted via super().write() by the time
        # _check_seo_write_permission() raises on the seo_vals half,
        # leaving the record with the non-SEO change committed and no
        # rollback inside this method. A caller passing both a non-SEO and
        # a SEO field in one vals dict reasonably expects the whole call to
        # succeed or fail as one unit.
        if seo_vals:
            self._check_seo_write_permission()

        res = True
        if other_vals:
            # Let standard Odoo ACLs handle non-SEO writes natively
            res = super().write(other_vals)

        if seo_vals:
            # Escalate strictly for the write operation using service acc
            # ADR-0001: Use with_context(mail_notrack=True)
            # LLM_EXPERIENCE: NEVER use prefetch_fields=False
            utils = self.env["zero_sudo.security.utils"]
            svc_uid = utils._get_service_uid(
                "user_websites.user_websites_service_account"
            )
            res = res and self.with_user(svc_uid).with_context(
                mail_notrack=True, skip_seo_metadata_mixin=True
            ).write(seo_vals)

        return res
