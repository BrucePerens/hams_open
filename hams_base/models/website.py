# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
from odoo import api, models


class Website(models.Model):
    _inherit = "website"

    # [@ANCHOR: hams_base:website_email_delivery_notice]
    @api.model
    def email_delivery_notice(self):
        """Plain text shown on /email-policy and /unsubscribe while outgoing email is limited (for example an AWS SES
        sandbox). Empty by default. Templates call this through the `website` object because /email-policy is served
        as a website page (so no controller supplies it a context) and a public visitor cannot read system
        parameters: the read goes through zero_sudo's whitelisted reader. Not a secret."""
        return self.env["zero_sudo.security.utils"]._get_system_param("hams_base.email_delivery_notice", "")
