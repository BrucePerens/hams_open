from odoo import models
from odoo.tools.translate import _
# -*- coding: utf-8 -*-
# from odoo import models, api, _
import html
import logging

_logger = logging.getLogger(__name__)

class ResPartner(models.Model):
    _inherit = "res.partner"

    # [@ANCHOR: hams_base:COMM_message_receive_bounce]
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
            # bug-hunt (2026-09-09): savepoint is load-bearing, not
            # decorative -- _get_service_uid()'s own SQL-backed uid lookup
            # does a real Postgres `RAISE EXCEPTION`
            # (zero_sudo_get_service_uid() in
            # zero_sudo/data/postgres_procedures.xml) on a missing/
            # disabled/non-service account, which aborts the CURRENT
            # transaction. This function returns immediately below on
            # failure, so it doesn't itself issue another SQL statement on
            # a poisoned transaction -- but `_message_receive_bounce` is
            # called from Odoo's own bounce-processing pipeline, which may
            # well do more DB work for the SAME message/partner batch in
            # the SAME transaction after this hook returns. Leaving the
            # transaction poisoned would silently break that unrelated
            # follow-on work too.
            with self.env.cr.savepoint():
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
        except Exception as e:  # audit-ignore-catch-all
            # bug-hunt (2026-09-09): this function's own docstring already
            # documents one real, previously-uncaught AccessError from
            # club.message_post() under the old (wrong) service account --
            # fixed by switching to zero_sudo.mail_service_internal, but the
            # except clause here was never widened to match. AccessError is
            # still not a KeyError/ValueError, so if the service account's
            # own ACLs ever regress, or _get_service_uid()/the SQL-backed
            # uid lookup fails for any other reason, this would again be an
            # uncaught crash in production bounce processing instead of a
            # logged, skipped notification. Class 20.
            _logger.exception("Failed to resolve clubs to notify of bounce for %s: %s", email, e)
            return

        for club in clubs_to_notify:
            # Prevent bounce loop: If the bouncing email IS the club email or a club officer email, do not notify them again.
            if club.email == email:
                continue

            # Adversarial security review, 2026-09-03: partner.name is
            # a normal user-editable field, and this body reaches
            # club.message_post() -- a backend chatter message any
            # club officer sees. Escaping before interpolation, same
            # defense-in-depth convention as event_sync's own scraped-
            # field fix earlier tonight, rather than trusting
            # message_post()'s own HTML sanitization to always catch
            # a raw '%'-interpolated payload.
            message = _(
                "System Alert: Email deliveries to member %(name)s (%(email)s) are bouncing. "
                "Please contact them via alternative means (phone, radio) to update their profile. "
                "If you need assistance, please submit a ticket at our Helpdesk: /helpdesk"
            ) % {'name': html.escape(partner.name or ''), 'email': html.escape(email or '')}

            try:
                # bug-hunt (2026-09-09): savepoint scoped per-club, not just
                # a bare try -- `message_post()` runs several DB statements
                # (mail.message insert, followers, notifications); a
                # failure partway through one club's post could otherwise
                # leave the transaction poisoned for the NEXT club's own
                # `message_post()` in this same loop, turning one club's
                # failure into every subsequent club's failure too (the
                # same cascading-isolation risk fixed in
                # `hams_base:res_users_write`).
                with self.env.cr.savepoint():
                    club.message_post(
                        body=message,
                        # bug-hunt (2026-09-09): this function's own comment
                        # above already escapes partner.name/email in `body`
                        # for defense-in-depth against a user-editable name
                        # reaching chatter; `subject` interpolated the same
                        # partner.name raw, three lines below that explicit
                        # review -- inconsistent within the same function.
                        # Escaped for the same reason, even though mail.message
                        # 'subject' is an ordinary Char (not Html) field.
                        subject=_("Bounce Alert: %(name)s") % {'name': html.escape(partner.name or '')},
                        message_type='notification',
                        subtype_xmlid='mail.mt_comment',
                    )
            except Exception as e:  # audit-ignore-catch-all
                # bug-hunt (2026-09-09): the try/except used to wrap the
                # WHOLE for-loop, not each club's own post -- one club's
                # message_post() failure (a bad attachment, a transient
                # DB issue on that specific club record, etc.) aborted the
                # loop entirely, silently skipping the notification for
                # every OTHER club in the same batch, not just the one
                # that failed. Scoped per-club so one club's failure can't
                # suppress another club's real notification.
                _logger.exception("Failed to notify club %s of bounce for %s: %s", club.id, email, e)
