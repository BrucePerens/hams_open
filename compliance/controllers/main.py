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

    # [@ANCHOR: compliance:dmca_page_route]
    @http.route('/compliance/dmca', type='http', auth='public', website=True)
    def compliance_dmca(self):
        """DMCA notice and designated-agent page.

        The registration number and the legal entity name are per-deployment: this repository is open source and other sites
        run it, so neither is written into the template. They come from the system parameters `compliance.dmca_registration_number`
        and `compliance.dmca_agent_name` (read through Zero-Sudo's whitelisted reader); the postal address, phone and email come
        from the company record, as the site footer does. A site with no registration number says so on the page.
        """
        # Bypass the distributed cache: a changed registration number must show at once, and this page is rarely requested.
        utils = http.request.env['zero_sudo.security.utils'].with_context(redis_bypass_cache=True)
        registration_number = (utils._get_system_param('compliance.dmca_registration_number', '') or '').strip()
        agent_name = (utils._get_system_param('compliance.dmca_agent_name', '') or '').strip()
        return http.request.render('compliance.dmca_template', {
            'dmca_registration_number': registration_number,
            'dmca_agent_name': agent_name,
        })
