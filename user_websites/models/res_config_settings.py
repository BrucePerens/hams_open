# SPDX-License-Identifier: AGPL-3.0-or-later
# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
import logging

from odoo import models, fields, api

_logger = logging.getLogger(__name__)


class ResConfigSettings(models.TransientModel):
    _inherit = "res.config.settings"

    user_websites_administrators_ids = fields.Many2many(
        "res.users",
        relation="settings_user_websites_admin_rel",
        string="User Websites Administrators",
        help="Users with full access to manage all user websites and groups.",
    )

    global_website_page_limit = fields.Integer(
        string="Global Page Limit",
        config_parameter="user_websites.global_website_page_limit",
        default=100,
        help="Default maximum number of web pages a standard user can create.",
    )

    company_abuse_email = fields.Char(
        string="Abuse Reporting Email",
        config_parameter="user_websites.company_abuse_email",
        help="Email address where content violation reports will be sent.",
    )

    @api.model
    # [@ANCHOR: user_websites:COMM_settings_get_values]
    def get_values(self):
        res = super(ResConfigSettings, self).get_values()
        admin_group = self.env.ref(
            "user_websites.group_user_websites_administrator", raise_if_not_found=False
        )
        if admin_group:
            svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                "user_websites.user_websites_service_account"
            )
            admin_users = [
                u.id for u in admin_group.with_user(svc_uid).user_ids if u.id != svc_uid
            ]
            res["user_websites_administrators_ids"] = [(6, 0, admin_users)]
        else:
            res["user_websites_administrators_ids"] = [(6, 0, [])]
        return res

    # [@ANCHOR: user_websites:COMM_settings_set_values]
    def set_values(self):
        super(ResConfigSettings, self).set_values()
        admin_group = self.env.ref(
            "user_websites.group_user_websites_administrator", raise_if_not_found=False
        )
        if admin_group:
            svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                "user_websites.user_websites_service_account"
            )
            new_ids = self.user_websites_administrators_ids.ids + [svc_uid]
            admin_group.with_user(svc_uid).write({"user_ids": [(6, 0, new_ids)]})
        else:
            # Unlike global_website_page_limit/company_abuse_email (plain
            # config_parameter fields super().set_values() already persisted
            # unconditionally above), the administrators list has no
            # fallback storage -- if the group's own XML data hasn't loaded
            # yet (a transient upgrade-ordering state; this group is meant
            # to always exist once the module is installed), this branch
            # used to silently drop the admin selection the caller just
            # made with the Settings screen still reporting success. Log it
            # so the gap is at least visible instead of fully silent.
            _logger.warning(
                "user_websites.group_user_websites_administrator not found; "
                "administrators selection from Settings was not saved."
            )
