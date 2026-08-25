from odoo import models
from odoo.tools.translate import _
# -*- coding: utf-8 -*-
# from odoo import models, api, _
import logging

_logger = logging.getLogger(__name__)

class ResPartner(models.Model):
    _inherit = "res.partner"

    def _message_receive_bounce(self, email, partner):
        """
        Override the native bounce handler to intercept bounces and notify
        club officers if the partner is a member of any clubs.

        Real hook name confirmed against the installed Odoo version's own mail_thread.py /
        mail_thread_blacklist.py, both of which define `_message_receive_bounce(self, email,
        partner)` -- no `mail_id` parameter, and the leading underscore is load-bearing: the
        previous non-underscored `message_receive_bounce(self, email, partner, mail_id=None)`
        here didn't override anything at all (Odoo's real bounce-processing pipeline only ever
        calls the underscored name), so this whole club-officer-notification feature silently
        never fired in production since it was written -- found only because a new test (see
        hams_base/tests/test_res_partner_bounce.py) called it directly and hit
        `AttributeError: 'super' object has no attribute 'message_receive_bounce'` on the old
        name, which does not exist anywhere in Odoo core either.
        """
        super()._message_receive_bounce(email, partner)
        
        if not partner:
            return

        # Attempt to find club relationships if ham_club_management is installed
        # In ham_club_management, partners might have a club_id or club_membership_ids
        try:
            # zero_sudo.mail_service_internal, not odoo_facility_service_internal: this method's
            # own point is to POST A MESSAGE, and odoo_facility_service_internal's real ACLs
            # (zero_sudo.kv, zero_sudo.security.log only -- see zero_sudo/security/
            # ir.model.access.csv) don't cover mail.message create at all. Confirmed directly,
            # not assumed: a real test calling this method hit a genuine, uncaught AccessError on
            # club.message_post() below (AccessError is neither KeyError nor ValueError, so the
            # except clause here would NOT have swallowed it -- this would have been a hard crash
            # in production bounce processing, not just a silently-missing notification).
            # mail_service_internal is this codebase's own established account for exactly this
            # purpose -- see ham_logbook/models/ham_qso.py's identical `mail_svc` pattern.
            svc_uid = self.env['zero_sudo.security.utils']._get_service_uid('zero_sudo.mail_service_internal')
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
