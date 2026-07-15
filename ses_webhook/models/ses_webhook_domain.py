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
        base_url = self.env['ir.config_parameter'].sudo().get_param('web.base.url')
        for record in self:
            if record.secret_token:
                record.webhook_url = f"{base_url}/mail/webhook/sns?token={record.secret_token}"
            else:
                record.webhook_url = ""

    _name_uniq = models.Constraint("UNIQUE(name)", "The domain name must be unique!")
    _token_uniq = models.Constraint("UNIQUE(secret_token)", "The secret token must be unique!")
