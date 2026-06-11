# -*- coding: utf-8 -*-
from odoo import models, fields, api


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
    def get_values(self):
        res = super(ResConfigSettings, self).get_values()
        admin_group = self.env.ref(
            "user_websites.group_user_websites_administrator", raise_if_not_found=False
        )
        if admin_group:
            svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                "zero_sudo.user_websites_service_account"
            )
            admin_users = admin_group.with_user(svc_uid).user_ids.ids
            res["user_websites_administrators_ids"] = [(6, 0, admin_users)]
        else:
            res["user_websites_administrators_ids"] = [(6, 0, [])]
        return res

    def set_values(self):
        super(ResConfigSettings, self).set_values()
        admin_group = self.env.ref(
            "user_websites.group_user_websites_administrator", raise_if_not_found=False
        )
        if admin_group:
            svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                "zero_sudo.user_websites_service_account"
            )
            admin_group.with_user(svc_uid).write(
                {"user_ids": [(6, 0, self.user_websites_administrators_ids.ids)]}
            )
