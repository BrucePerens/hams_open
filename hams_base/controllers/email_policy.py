# -*- coding: utf-8 -*-
from odoo import http

class EmailPolicyController(http.Controller):
    # [@ANCHOR: hams_base:COMM_email_policy_route]
    @http.route('/email-policy', type='http', auth='public', website=True)
    def email_policy(self, **kw):
        return http.request.render('hams_base.email_policy_template')
