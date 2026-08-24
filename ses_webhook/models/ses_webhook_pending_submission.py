# SPDX-License-Identifier: AGPL-3.0-or-later
import logging
import secrets

from dateutil.relativedelta import relativedelta

from odoo import _, api, fields, models

_logger = logging.getLogger(__name__)


class SesWebhookPendingSubmission(models.Model):
    """docs/proposals/SES_WEBHOOK_SENDER_REGISTRATION.md section 3: holds an
    unregistered/unmatched sender's original inbound email content while
    they're nudged toward registering, rather than either silently
    creating a misattributed record (the old behavior) or silently
    dropping the message. Deliberately narrower retention than
    ses.webhook.log's 30-day window (see _cron_truncate_pending_submissions)
    since this table holds actual unauthenticated email content, not just
    success/failure metadata.

    Linking a since-registered user back to their pending submission via
    `token` is section 3's own explicitly-flagged open sub-question, not
    resolved here -- `consumed` exists as the field that flow would set,
    but nothing sets it yet.
    """

    _name = 'ses.webhook.pending_submission'
    _description = 'SES Webhook: Pending Registration-Gated Submission'
    _order = 'create_date desc'

    name = fields.Char(string='Name', compute='_compute_name', store=True)
    sender_email = fields.Char(string='Sender Email', required=True, index=True, readonly=True)
    raw_content = fields.Text(string='Raw Email Content', readonly=True)
    domain_id = fields.Many2one('ses.webhook.domain', string='Webhook Domain', readonly=True, ondelete='cascade')
    company_id = fields.Many2one('res.company', related='domain_id.company_id', store=True, readonly=True)
    token = fields.Char(
        string='Token', required=True, copy=False, readonly=True,
        default=lambda self: secrets.token_urlsafe(24),
    )
    consumed = fields.Boolean(string='Consumed', default=False, readonly=True)

    _token_uniq = models.Constraint("UNIQUE(token)", "The token must be unique!")

    @api.depends('sender_email')
    def _compute_name(self):
        for record in self:
            record.name = _('Pending submission from %s', record.sender_email) if record.sender_email else _('New')

    @api.model
    def _cron_truncate_pending_submissions(self):
        """Deletes pending submissions older than a 7-day retention window,
        regardless of consumed state -- both "never registered" and
        "already claimed and no longer needed" end the same way. See
        SES_WEBHOOK_SENDER_REGISTRATION.md section 3 for why 7 days
        (long enough to register and notice the confirmation email, short
        enough this isn't a quiet second copy of someone's message living
        indefinitely) rather than reusing ses.webhook.log's 30-day number.
        """
        cutoff_date = fields.Datetime.now() - relativedelta(days=7)
        old = self.env["ses.webhook.pending_submission"].search(
            [('create_date', '<', cutoff_date)], limit=10000
        )
        if old:
            old.unlink()

    @api.model
    def create_and_notify(self, sender_email, raw_content, domain):
        """SES_WEBHOOK_SENDER_REGISTRATION.md sections 1-4: creates the
        pending-submission record for an unmatched sender and sends a
        real, specific registration nudge -- never silence. The reply is
        a narrow, purpose-built mail.mail send (section 4's resolved
        decision), not routed through mail.thread's reply/alias
        infrastructure, so it doesn't reopen the privilege question
        webhook_api.py's own message_process() call site already settled.
        Runs entirely under the caller's already-elevated ses_webhook
        service account -- no .sudo() here.
        """
        submission = self.env['ses.webhook.pending_submission'].create({
            'sender_email': sender_email,
            'raw_content': raw_content,
            'domain_id': domain.id,
        })

        base_url = self.env['zero_sudo.security.utils']._get_system_param('web.base.url')
        signup_url = f"{base_url}/web/signup"
        body_html = _(
            '<p>Hello,</p>'
            '<p>We received your email to <strong>%(domain)s</strong>, but couldn\'t match it to a '
            'registered account, so no ticket was filed yet.</p>'
            '<p>To file this as a real user, please '
            '<a href="%(signup_url)s">create an account</a> using this same email address, then '
            'resend your original message.</p>'
        ) % {'domain': domain.name, 'signup_url': signup_url}

        mail = self.env['mail.mail'].create({
            'subject': _('Registration required to file a request'),
            'body_html': body_html,
            'email_to': sender_email,
            'email_from': domain.company_id.catchall_formatted or domain.company_id.email_formatted or None,
        })
        mail.send()

        return submission
