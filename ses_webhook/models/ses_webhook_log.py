# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import fields, models

class SesWebhookLog(models.Model):
    _name = 'ses.webhook.log'
    _description = 'SES Webhook Log'
    _order = 'create_date desc'

    name = fields.Char(string='Message ID', required=True, readonly=True)
    payload_type = fields.Selection([
        ('Notification', 'Notification'),
        ('SubscriptionConfirmation', 'Subscription Confirmation'),
        ('UnsubscribeConfirmation', 'Unsubscribe Confirmation'),
        ('Unknown', 'Unknown')
    ], string='Payload Type', readonly=True)
    status = fields.Selection([
        ('success', 'Success'),
        ('failed', 'Failed'),
        ('ignored', 'Ignored'),
        ('rejected_subscribe_url', 'Rejected Subscribe URL'),
        ('rejected_signature', 'Rejected: Invalid AWS SNS Signature'),
    ], string='Status', readonly=True)
    domain_id = fields.Many2one('ses.webhook.domain', string='Webhook Domain', readonly=True, ondelete='cascade')
    company_id = fields.Many2one('res.company', related='domain_id.company_id', store=True, readonly=True)
    error_message = fields.Text(string='Error Message', readonly=True)
    raw_payload = fields.Text(string='Raw JSON Payload', readonly=True)
