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
            # Core Odoo's res.users.write() refuses `active=False` when
            # self.env.uid is in the ids being written -- i.e. it always
            # blocks a user from deactivating their own live session,
            # raising UserError. Writing through user.sudo() (as this used
            # to) doesn't help: sudo() only bypasses ACLs, it leaves
            # env.uid unchanged, so that guard still fires and this route
            # always returned a 422 error instead of ever locking anyone
            # out. Routing the write through a dedicated zero-sudo service
            # account works around it correctly: that account's own uid
            # differs from the target user's, so the guard doesn't fire.
            # See zero_sudo/data/security_data.xml's
            # user_lockout_service_internal / group_account_lockout_service
            # and this module's own hams_base/security/ir.model.access.csv
            # for the (scoped, res.users-only, no create/unlink) grant.
            svc_uid = request.env["zero_sudo.security.utils"]._get_service_uid(
                "zero_sudo.user_lockout_service_internal"
            )
            user.with_user(svc_uid).write({'active': False})

            # Additional cleanup: revoke portal access, etc. can be done here.
            # Log out the user
            request.session.logout(keep_db=True)
            return request.render('hams_base.unsubscribe_lockout_success', {})
        return request.redirect('/')
