# -*- coding: utf-8 -*-
# Copyright (c) 2024 Bruce Perens K6BP
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import http

class ComplianceController(http.Controller):
    # [@ANCHOR: COMM_compliance_index_route]
    @http.route('/compliance', type='http', auth='public', website=True)
    def compliance_index(self):
        env = http.request.env

        # Bug-hunt finding: compliance.document has no company_id field at
        # all, so the .with_company(env.company) that used to sit here was
        # a complete no-op -- it implied per-company legal-document scoping
        # that doesn't exist anywhere in this model (no company_id field,
        # no ir.rule referencing one). Removed rather than "fixed" into a
        # real filter: these documents (Privacy Policy, Cookie Policy,
        # Terms of Service, Accessibility Statement) are intentionally
        # global platform-wide legal notices, not per-company data: adding
        # real company scoping would be a product decision, not a bug fix.
        domain = [('active', '=', True)]
        docs = env['compliance.document'].search(domain, limit=100)

        return http.request.render('compliance.compliance_index_template', {'docs': docs})
