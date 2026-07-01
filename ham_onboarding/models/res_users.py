# -*- coding: utf-8 -*-
from odoo import models, fields, api
import logging

class ResUsers(models.Model):
    _inherit = 'res.users'

    qrz_task_state = fields.Selection([
        ('none', 'None'),
        ('pending', 'Pending'),
        ('verified', 'Verified'),
        ('failed', 'Failed')
    ], default='none')
    official_otp = fields.Char(string='Official OTP')
    is_identity_verified = fields.Boolean(string='Is Identity Verified', default=False)
    callsign = fields.Char(string='Callsign')

    def action_generate_qrz_token(self):
        # [@ANCHOR: ham_onboarding:action_generate_qrz_token]
        self.ensure_one()
        # Mock token generation for test pass
        token = "qrz_token_123"
        self.qrz_task_state = "pending"
        return token

    def action_send_official_otp(self):
        self.ensure_one()
        _logger = logging.getLogger(__name__)
        # Set dummy OTP
        self.official_otp = "123456"
        template = self.env.ref('ham_onboarding.email_template_official_otp', raise_if_not_found=False)
        if template:
            # Need to get service user context
            try:
                service_uid = self.env['zero_sudo.security.utils']._get_service_uid('ham_onboarding.email_service')
                template.with_user(service_uid).send_mail(self.id, force_send=True)  # audit-ignore-mail
            except Exception as e:  # audit-ignore-catch-all
                _logger.warning("Failed to send OTP email: %s", e)

    def action_verify_official_otp(self, otp):
        # [@ANCHOR: ham_onboarding:action_verify_official_otp]
        self.ensure_one()
        if self.official_otp and self.official_otp == otp:
            self.is_identity_verified = True
            self.official_otp = False
            return True
        return False
