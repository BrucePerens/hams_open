from odoo import models
from odoo.tools.translate import _
# -*- coding: utf-8 -*-
# from odoo import models, api, _
import logging

_logger = logging.getLogger(__name__)

class ResPartner(models.Model):
    _inherit = "res.partner"

    def message_receive_bounce(self, email, partner, mail_id=None):
        """
        Override the native bounce handler to intercept bounces and notify 
        club officers if the partner is a member of any clubs.
        """
        super().message_receive_bounce(email, partner, mail_id=mail_id)
        
        if not partner:
            return

        # Attempt to find club relationships if ham_club_management is installed
        # In ham_club_management, partners might have a club_id or club_membership_ids
        try:
            # We use the odoo_facility_service_internal service account to bypass access rights for system-level notifications
            svc_uid = self.env['zero_sudo.security.utils']._get_service_uid('zero_sudo.odoo_facility_service_internal')
            partner_sudo = partner.with_user(svc_uid)
            clubs_to_notify = self.env['res.partner'] # empty recordset

            if 'club_ids' in partner_sudo._fields:
                clubs_to_notify = partner_sudo.club_ids
            elif partner_sudo.parent_id and partner_sudo.parent_id.is_company:
                clubs_to_notify = partner_sudo.parent_id
            
            for club in clubs_to_notify:
                # Prevent bounce loop: If the bouncing email IS the club email or a club officer email, do not notify them again.
                if club.email == email:
                    continue

                message = _(
                    "System Alert: Email deliveries to member %(name)s (%(email)s) are bouncing. "
                    "Please contact them via alternative means (phone, radio) to update their profile. "
                    "If you need assistance, please submit a ticket at our Helpdesk: /helpdesk"
                ) % {'name': partner.name, 'email': email}
                
                club.message_post(
                    body=message,
                    subject=_("Bounce Alert: %(name)s") % {'name': partner.name},
                    message_type='notification',
                    subtype_xmlid='mail.mt_comment',
                )
        except (KeyError, ValueError) as e:  # audit-ignore-catch-all
            _logger.exception("Failed to notify club officers of bounce for %s: %s", email, e)
