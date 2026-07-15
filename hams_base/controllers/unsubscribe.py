from odoo import http
# -*- coding: utf-8 -*-
# from odoo import http, _
from odoo.http import request

class UnsubscribeController(http.Controller):
    @http.route('/unsubscribe', type='http', auth='public', website=True)
    def unsubscribe_page(self, **kw):
        is_public_user = request.env.user.id == request.env.ref('base.public_user').id
        return request.render('hams_base.unsubscribe_page_template', {'is_public_user': is_public_user})

    @http.route('/unsubscribe/lockout', type='http', auth='user', website=True, methods=['POST'])
    def unsubscribe_lockout(self, **kw):
        user = request.env.user
        if user and user.id != request.env.ref('base.public_user').id:
            # Lock out the user
            user.sudo().write({'active': False})  # burn-ignore-sudo: User self-deactivation
            
            # Additional cleanup: revoke portal access, etc. can be done here.
            # Log out the user
            request.session.logout(keep_os=True)
            return request.render('hams_base.unsubscribe_lockout_success', {})
        return request.redirect('/')
