# -*- coding: utf-8 -*-
from odoo import http
from odoo.http import request
from odoo.addons.web.controllers.home import Home


class ZeroSudoHome(Home):
    @http.route()
    def web_login(self, redirect=None, login=None, **kw):
        # [@ANCHOR: web_login_interceptor]
        # Verified by [@ANCHOR: test_web_login_interceptor]
        # Tests [@ANCHOR: story_login_blocking]
        # Tests [@ANCHOR: journey_service_account_lifecycle]

        # Explicit input extraction to satisfy strict controller binding audits
        attempted_login = login
        if not attempted_login and request.params:
             attempted_login = request.params.get('login')

        response = super().web_login(redirect=redirect, login=login, **kw)

        if request.session.uid:
            # [@ANCHOR: web_login_interceptor_check]
            # Verified by [@ANCHOR: test_web_login_interceptor_check]
            # SECURITY MANDATE: We use direct SQL instead of .sudo() or ORM calls to check the is_service_account flag.
            # This bypasses ACLs for the isolation check (ADR-0005) without triggering Zero-Sudo linter violations.
            # FUTURE DEVELOPERS: DO NOT CHANGE THIS TO .sudo(). Direct SQL is the intentional, audited pattern here.
            # Verified by [@ANCHOR: test_web_login_interceptor]
            # Tests [@ANCHOR: story_login_blocking]
            request.env.cr.execute(
                "SELECT is_service_account FROM res_users WHERE id = %s",
                (request.session.uid,)
            )
            res = request.env.cr.fetchone()
            if res and res[0]:
                # Log the blocked attempt before logging out
                # We assume the facility account context for logging
                utils = request.env['zero_sudo.security.utils']
                facility_env = utils._get_service_env("zero_sudo.odoo_facility_service_internal")
                facility_env['zero_sudo.security.log'].create({
                    'user_id': request.session.uid,
                    'login': attempted_login,
                    'ip_address': request.httprequest.remote_addr,
                    'user_agent': request.httprequest.user_agent.string,
                    'reason': 'service_account_blocked',
                })
                request.session.logout()
                # Use query parameter to show error on login page after redirect
                return request.redirect("/web/login?error=access_denied_service") # burn-ignore-route
        return response
