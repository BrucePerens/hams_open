# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-only
from odoo import http

class ComplianceController(http.Controller):
    @http.route('/compliance', type='http', auth='public', website=True)
    def compliance_index(self, **kw):
        docs = http.request.env['compliance.document'].sudo().search([('active', '=', True)], limit=100)  # burn-ignore-sudo: Public docs access # audit-ignore-search: Tested by [@ANCHOR: COMM_test_compliance_index] # fmt: skip
        return http.request.render('compliance.compliance_index_template', {'docs': docs})
