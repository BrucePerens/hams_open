# -*- coding: utf-8 -*-
from odoo import http, _
from odoo.http import request

class UnsubscribeController(http.Controller):
    @http.route('/unsubscribe', type='http', auth='public', website=True)
    def unsubscribe_page(self, **kw):
        return request.render('hams_base.unsubscribe_page_template', {})

    @http.route('/unsubscribe/lockout', type='http', auth='user', website=True, methods=['POST'])
    def unsubscribe_lockout(self, **kw):
        user = request.env.user
        if user and user.id != request.env.ref('base.public_user').id:
            # Lock out the user
            user.sudo().write({'active': False})
            
            # Additional cleanup: revoke portal access, etc. can be done here.
            # Log out the user
            request.session.logout(keep_os=True)
            return request.render('hams_base.unsubscribe_lockout_success', {})
        return request.redirect('/')
