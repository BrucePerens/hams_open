# SPDX-License-Identifier: AGPL-3.0-or-later
import secrets
from odoo import api, fields, models

class SesWebhookDomain(models.Model):
    _name = 'ses.webhook.domain'
    _description = 'SES Webhook Domain Configuration'
    
    name = fields.Char(string='Domain Name', required=True, help="E.g., example.com")
    secret_token = fields.Char(string='Secret Token', required=True, copy=False, default=lambda self: secrets.token_urlsafe(24))
    company_id = fields.Many2one('res.company', string='Tenant (Company)', required=True, default=lambda self: self.env.company)
    webhook_url = fields.Char(string='Webhook URL', compute='_compute_webhook_url')
    log_ids = fields.One2many('ses.webhook.log', 'domain_id', string='Logs')
    
    @api.depends('secret_token')
    def _compute_webhook_url(self):
        # .sudo() is forbidden on this platform. base.group_user has no
        # direct ir.config_parameter access, but any base.group_user can
        # view this record (see ir.model.access.csv), so this compute must
        # read web.base.url without depending on the viewer's own
        # privilege -- _get_system_param() is the Zero-Sudo-vetted path
        # for exactly that, and web.base.url is already on its whitelist.
        base_url = self.env['zero_sudo.security.utils']._get_system_param('web.base.url')
        for record in self:
            if record.secret_token:
                record.webhook_url = f"{base_url}/mail/webhook/sns?token={record.secret_token}"
            else:
                record.webhook_url = ""

    _name_uniq = models.Constraint("UNIQUE(name)", "The domain name must be unique!")
    _token_uniq = models.Constraint("UNIQUE(secret_token)", "The secret token must be unique!")

    @api.model_create_multi
    def create(self, vals_list):
        records = super().create(vals_list)
        records._sync_service_account_companies()
        return records

    def write(self, vals):
        res = super().write(vals)
        if 'company_id' in vals:
            self._sync_service_account_companies()
        return res

    def _sync_service_account_companies(self):
        # webhook_api.py's service account (with_user(), in place of the
        # .sudo() this used to need) must be a company_ids member of every
        # tenant with a configured domain, or with_company(domain.company_id)
        # raises AccessError inside message_process() for any company that
        # isn't -- Environment.company/.companies validate allowed_company_ids
        # against the acting user's own company_ids whenever the environment
        # isn't sudoed (odoo/orm/environments.py). Keeps that membership
        # growing with real tenant onboarding; never a hardcoded count.
        svc_uid = self.env['zero_sudo.security.utils']._get_service_uid(
            'ses_webhook.user_ses_webhook_service_internal'
        )
        svc_user = self.env['res.users'].browse(svc_uid)
        missing = self.mapped('company_id') - svc_user.company_ids
        if missing:
            svc_user.write({'company_ids': [(4, c.id) for c in missing]})
