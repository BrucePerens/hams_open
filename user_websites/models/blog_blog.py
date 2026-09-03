# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).
from odoo import models, api, fields, _
from odoo.exceptions import AccessError, ValidationError


class BlogBlog(models.Model):
    _name = "blog.blog"
    name = fields.Char(string="Name", default=lambda self: self._description)
    _inherit = ["blog.blog", "user_websites.owned.mixin"]

    _name_owner_uniq = models.Constraint("UNIQUE(name, owner_user_id, user_websites_group_id)", "You already have a blog with this exact title!")

    def _check_blog_quota(self, vals_list):
        # [@ANCHOR: user_websites_blog_quota_check]
        # Adversarial security review, 2026-09-03: unlike website.page
        # ([@ANCHOR: website_page_quota_check]), blog.blog create() had no
        # quota at all -- any authenticated user could create unbounded
        # blog containers via direct RPC. Real default is small (5) since
        # a real user typically needs very few of these, unlike posts.
        # Deliberately no su/admin exemption here, matching
        # website.page's own established _get_page_limit() check exactly
        # -- that check is keyed purely on the owner_user_id field in the
        # payload, not on who the RPC caller is, and adding an exemption
        # here would be new, undiscussed behavior beyond what this
        # proposal's own fix is meant to close.
        owner_ids = [
            vals.get("owner_user_id") for vals in vals_list if vals.get("owner_user_id")
        ]
        if not owner_ids:
            return
        unique_owner_ids = list(set(owner_ids))
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_websites_service_account"
        )
        users = self.env["res.users"].with_user(svc_uid).browse(unique_owner_ids)
        limits = {user.id: user._get_blog_limit() for user in users}

        existing_counts = {u_id: 0 for u_id in unique_owner_ids}
        for owner, count in (
            self.env["blog.blog"]
            .with_user(svc_uid)
            ._read_group([("owner_user_id", "in", unique_owner_ids)], ["owner_user_id"], ["__count"])
        ):
            existing_counts[owner.id] = count

        batch_counts = {u_id: 0 for u_id in unique_owner_ids}
        for vals in vals_list:
            o_id = vals.get("owner_user_id")
            if o_id:
                batch_counts[o_id] += 1

        for o_id in unique_owner_ids:
            if existing_counts[o_id] + batch_counts[o_id] > limits[o_id]:
                raise ValidationError(
                    _("You have reached your limit of %s blogs.") % limits[o_id]
                )

    @api.model_create_multi
    def create(self, vals_list):
        self._check_proxy_ownership_create(vals_list)
        self._check_blog_quota(vals_list)
        if not (
            self.env.su
            or self.env.user.has_group("base.group_system")
            or self.env.user.has_group(
                "user_websites.group_user_websites_administrator"
            )
        ):
            allowed = {
                "name",
                "subtitle",
                "owner_user_id",
                "user_websites_group_id",
                "website_id",
                "website_meta_title",
                "website_meta_description",
                "website_meta_keywords",
                "website_meta_og_img",
                "seo_name",
            }
            for vals in vals_list:
                for k in list(vals.keys()):
                    if k not in allowed:
                        del vals[k]

        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_websites_service_account"
        )
        self_svc = self.with_user(svc_uid).with_context(mail_notrack=True)
        return super(BlogBlog, self_svc).create(vals_list)

    def check_access(self, operation):
        """Proactively catch write/unlink access violations to prevent ir.rule INFO log spam."""
        if operation in ("write", "unlink") and not self.env.su and self:
            if self.env.user.has_group(
                "user_websites.group_user_websites_user"
            ) and not self.env.user.has_group(
                "user_websites.group_user_websites_administrator"
            ):
                user_id = self.env.user.id
                group_ids = self.mapped("user_websites_group_id").ids
                member_map = {}
                if group_ids:
                    svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                        "user_websites.user_websites_service_account"
                    )
                    groups = (
                        self.env["user.websites.group"]
                        .with_user(svc_uid)
                        .browse(group_ids)
                    )
                    for g in groups:
                        member_map[g.id] = set(g.member_ids.ids)

                for blog in self:
                    is_owner = blog.owner_user_id.id == user_id
                    is_group_member = (
                        blog.user_websites_group_id
                        and user_id
                        in member_map.get(blog.user_websites_group_id.id, set())
                    )
                    if not is_owner and not is_group_member:

                        raise AccessError(
                            _(
                                "Access Denied: You do not have permission to modify this blog."
                            )
                        )
        return super(BlogBlog, self).check_access(operation)

    def write(self, vals):
        self.check_access("write")
        self._check_proxy_ownership_write(vals)
        if not (
            self.env.su
            or self.env.user.has_group("base.group_system")
            or self.env.user.has_group(
                "user_websites.group_user_websites_administrator"
            )
        ):
            allowed = {
                "name",
                "subtitle",
                "owner_user_id",
                "user_websites_group_id",
                "website_id",
                "website_meta_title",
                "website_meta_description",
                "website_meta_keywords",
                "website_meta_og_img",
                "seo_name",
            }
            for k in list(vals.keys()):
                if k not in allowed:
                    del vals[k]

        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_websites_service_account"
        )
        self_svc = self.with_user(svc_uid).with_context(mail_notrack=True)
        return super(BlogBlog, self_svc).write(vals)

    def unlink(self):
        self.check_access("unlink")
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_websites_service_account"
        )
        self_svc = self.with_user(svc_uid).with_context(mail_notrack=True)
        return super(BlogBlog, self_svc).unlink()
