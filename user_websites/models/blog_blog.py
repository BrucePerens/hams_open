# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
from odoo import models, api, _
from odoo.exceptions import AccessError

class BlogBlog(models.Model):
    _name = "blog.blog"
    _inherit = ["blog.blog", "user_websites.owned.mixin"]

    _name_owner_uniq = models.Constraint(
        "UNIQUE(name, owner_user_id, user_websites_group_id)",
        "You already have a blog with this exact title!",
    )

    @api.model_create_multi
    def create(self, vals_list):
        self._check_proxy_ownership_create(vals_list)
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
            }
            for vals in vals_list:
                for k in list(vals.keys()):
                    if k not in allowed:
                        del vals[k]

        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_user_websites_service_account"
        )
        return super(BlogBlog, self.with_user(svc_uid)).create(vals_list)

    def check_access_rule(self, operation):
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
                        "user_websites.user_user_websites_service_account"
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
        return super(BlogBlog, self).check_access_rule(operation)

    def write(self, vals):
        self.check_access_rights("write")
        self.check_access_rule("write")
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
            }
            for k in list(vals.keys()):
                if k not in allowed:
                    del vals[k]

        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_user_websites_service_account"
        )
        return super(BlogBlog, self.with_user(svc_uid)).write(vals)

    def unlink(self):
        self.check_access_rights("unlink")
        self.check_access_rule("unlink")
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_user_websites_service_account"
        )
        return super(BlogBlog, self.with_user(svc_uid)).unlink()
