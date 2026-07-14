# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-only
from odoo import http

class ComplianceController(http.Controller):
    @http.route('/compliance', type='http', auth='public', website=True)
    def compliance_index(self, **kw):
        svc_user = http.request.env.ref("compliance.user_compliance_service")
        docs = http.request.env['compliance.document'].with_user(svc_user).search([('active', '=', True)], limit=100)
        return http.request.render('compliance.compliance_index_template', {'docs': docs})
