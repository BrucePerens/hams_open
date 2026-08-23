# SPDX-License-Identifier: AGPL-3.0-or-later
import logging
import json
import urllib.request
from odoo import http
from odoo.http import request

_logger = logging.getLogger(__name__)

class SesWebhookController(http.Controller):
    
    @http.route('/mail/webhook/sns', type='http', auth='public', methods=['POST'], csrf=False)
    def receive_sns_webhook(self, **kwargs):
        """
        Receives Amazon SNS webhooks for incoming SES emails.
        Validates the secret token against configured domains,
        and routes to the appropriate tenant company.
        """
        raw_data = request.httprequest.get_data(as_text=True)
        token = request.httprequest.args.get('token')
        
        if not token:
            _logger.warning("SES Webhook denied: Missing token.")
            return request.make_response("Forbidden", status=403)

        # Service account, in place of the .sudo() this used to need. It
        # holds both base.group_user and its own dedicated group, which
        # carries a second, unconditional ir.rule
        # (ses_webhook_domain_rule_svc / ses_webhook_log_rule_svc) -- Odoo
        # ORs together the domains of every group-scoped rule a user
        # matches, so this account's effective access is unconditional
        # cross-tenant, with no company enumerated anywhere. See
        # security/ses_webhook_security.xml and
        # models/ses_webhook_domain.py's _sync_service_account_companies()
        # for the company_ids-growth mechanism this depends on for
        # with_company() below.
        svc_uid = request.env['zero_sudo.security.utils']._get_service_uid(
            'ses_webhook.user_ses_webhook_service_internal'
        )

        # 1. Validate Secret Token against configured domains. This lookup
        # runs BEFORE the requesting company is known (the token is what
        # determines it), so it must see every tenant's ses.webhook.domain
        # record regardless of company -- exactly what the service
        # account's unconditional rule provides.
        domain = request.env['ses.webhook.domain'].with_user(svc_uid).search([('secret_token', '=', token)], limit=1)
        if not domain:
            _logger.warning("SES Webhook denied: Invalid token.")
            return request.make_response("Forbidden", status=403)
            
        if not raw_data or not raw_data.strip():
            return request.make_response("Empty payload", status=400)
            
        try:
            payload = json.loads(raw_data)
        except json.JSONDecodeError:
            return request.make_response("Invalid JSON", status=400)

        payload_type = payload.get('Type', 'Unknown')
        message_id = payload.get('MessageId', 'Unknown')
        
        # Create Log Record
        log_vals = {
            'name': message_id,
            'payload_type': payload_type,
            'raw_payload': raw_data,
            'domain_id': domain.id,
        }

        try:
            if payload_type == 'SubscriptionConfirmation':
                subscribe_url = payload.get('SubscribeURL')
                if subscribe_url:
                    urllib.request.urlopen(subscribe_url)
                    _logger.info("Successfully confirmed SNS subscription for domain %s.", domain.name)
                    log_vals.update({'status': 'success'})
                    
            elif payload_type == 'Notification':
                ses_message_str = payload.get('Message', '{}')
                ses_message = json.loads(ses_message_str)
                
                raw_email = ses_message.get('content')
                
                if not raw_email:
                    _logger.warning("SES Webhook received Notification with no 'content' field for domain %s.", domain.name)
                    log_vals.update({'status': 'ignored', 'error_message': 'No content field found.'})
                else:
                    email_bytes = raw_email.encode('utf-8')
                    # RESOLVED (user decision): restore .sudo() here, with
                    # an explicit bypass tag. Verified directly against
                    # Odoo 19's own mail_thread.py (_message_route_process)
                    # that a bare, unsudoed call here achieves NO privilege
                    # reduction at all -- `ModelCtx.sudo()` fires
                    # unconditionally whenever an alias matched (true by
                    # construction in this branch), so the record write
                    # already runs under full ACL bypass either way. What
                    # removing .sudo() actually did was break sender
                    # attribution: `if self.env.is_system(): ModelCtx =
                    # Model.with_user(related_user)` -- the branch that
                    # attributes the record to the real matched sender --
                    # is gated on the calling env holding su or
                    # base.group_system, neither of which this public
                    # route's env holds without .sudo() here. Restoring it
                    # fixes that regression at no additional privilege cost
                    # (mail_thread.py was always going to elevate
                    # internally regardless) -- the alternative,
                    # granting the service account base.group_system
                    # itself just to satisfy is_system(), would have been a
                    # much bigger and genuinely worse grant.
                    # This also means with_company()'s own AccessError
                    # (its docstring: "may trigger an AccessError if not
                    # done in a sudoed environment") can no longer fire
                    # here, so _sync_service_account_companies()'s
                    # company_ids-growth mechanism (built to dodge exactly
                    # that error) is now confirmed dead weight for this
                    # specific call path -- not removed here, a separate
                    # follow-on cleanup if wanted.
                    request.env['mail.thread'].sudo().with_company(domain.company_id).message_process(None, email_bytes)  # burn-ignore-sudo: verified no additional privilege vs. mail_thread.py's own unconditional internal sudo() on any alias match; restores correct sender attribution. See night_shift_todo.md.
                    _logger.info("Successfully processed incoming email from SNS Webhook for domain %s.", domain.name)
                    log_vals.update({'status': 'success'})
                    
            elif payload_type == 'UnsubscribeConfirmation':
                _logger.info("Received UnsubscribeConfirmation for domain %s.", domain.name)
                log_vals.update({'status': 'ignored'})
            else:
                log_vals.update({'status': 'ignored', 'error_message': 'Unknown payload type'})
                
        # Must return 200 to AWS regardless of what fails inside (a
        # non-2xx response makes SNS retry indefinitely) -- any failure
        # is logged to ses.webhook.log with status='failed' instead.
        except Exception as e:  # audit-ignore-catch-all: Tested by [@ANCHOR: ses_webhook_process_catch_all]  # fmt: skip
            _logger.error("Failed to process SNS Webhook: %s", str(e))
            log_vals.update({'status': 'failed', 'error_message': str(e)})
            
        finally:
            # log_vals['company_id'] is a related field computed from
            # domain_id.company_id, which can be any tenant company -- the
            # same cross-tenant access the domain lookup above needed,
            # provided by the same service account's unconditional rule.
            request.env['ses.webhook.log'].with_user(svc_uid).create(log_vals)

        return request.make_response("OK", status=200)
