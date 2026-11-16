# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import api, fields, models

# [@ANCHOR: ses_webhook_ses_webhook_log_ses_webhook_log]
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
        ('rejected_topic', 'Rejected: Wrong SNS Topic'),
        ('rejected_sender', 'Rejected: Sender Failed SES Authentication'),
    ], string='Status', readonly=True)
    # The audit rows are kept when their domain is deleted (Bruce, 2026-11-16); company_id is a stored copy, so a kept row
    # stays visible to its company (a compute that keeps its value, not a related field, which the delete would clear), and domain_name still says which domain it was.
    domain_id = fields.Many2one('ses.webhook.domain', string='Webhook Domain', readonly=True, ondelete='set null', index=True)
    domain_name = fields.Char(string='Domain Name', compute='_compute_domain_name', store=True, readonly=True)
    company_id = fields.Many2one('res.company', compute='_compute_company_id', store=True, readonly=True)
    error_message = fields.Text(string='Error Message', readonly=True)
    raw_payload = fields.Text(string='Raw JSON Payload', readonly=True)

    @api.depends('domain_id.company_id')
    def _compute_company_id(self):
        for log in self:
            if log.domain_id:
                log.company_id = log.domain_id.company_id

    # [@ANCHOR: ses_webhook:log_compute_domain_name]
    # Verified by [@ANCHOR: ses_webhook:test_log_survives_domain_delete]
    @api.depends('domain_id.name')
    def _compute_domain_name(self):
        for log in self:
            if log.domain_id:
                log.domain_name = log.domain_id.name

    # [@ANCHOR: ses_webhook:already_handled]
    @api.model
    def _already_handled(self, domain_id, message_id):
        """True when this MessageId was already processed for the domain (a failed one does not count: it is reprocessed).

        The log row is written after the message is processed, so a search alone lets two simultaneous deliveries both
        process it. A no-change UPDATE of the domain row makes them take turns: the later one fails to serialize (its
        snapshot cannot see the earlier log) and Odoo runs the request again, when the search finds the log. There is no
        unique index to lean on: failed and rejected attempts of one MessageId are logged too."""
        self.env.cr.execute("UPDATE ses_webhook_domain SET id = id WHERE id = %s", [domain_id])
        return bool(self.search_count([
            ('name', '=', message_id),
            ('domain_id', '=', domain_id),
            ('status', 'in', ('success', 'ignored')),
        ], limit=1))
