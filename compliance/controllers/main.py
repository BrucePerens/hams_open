# -*- coding: utf-8 -*-
# Copyright (c) 2024 Bruce Perens K6BP
# SPDX-License-Identifier: AGPL-3.0-only
from odoo import http

class ComplianceController(http.Controller):
    # [@ANCHOR: COMM_compliance_index_route]
    @http.route('/compliance', type='http', auth='public', website=True)
    def compliance_index(self):
        env = http.request.env
        
        domain = [('active', '=', True)]
        docs = env['compliance.document'].with_company(env.company).search(domain, limit=100)
        
        return http.request.render('compliance.compliance_index_template', {'docs': docs})
