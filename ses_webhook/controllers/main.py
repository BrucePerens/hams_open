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

        # 1. Validate Secret Token against configured domains
        domain = request.env['ses.webhook.domain'].sudo().search([('secret_token', '=', token)], limit=1)
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
                    # Route to the specific tenant company
                    request.env['mail.thread'].with_company(domain.company_id).sudo().message_process(None, email_bytes)
                    _logger.info("Successfully processed incoming email from SNS Webhook for domain %s.", domain.name)
                    log_vals.update({'status': 'success'})
                    
            elif payload_type == 'UnsubscribeConfirmation':
                _logger.info("Received UnsubscribeConfirmation for domain %s.", domain.name)
                log_vals.update({'status': 'ignored'})
            else:
                log_vals.update({'status': 'ignored', 'error_message': 'Unknown payload type'})
                
        except Exception as e:
            _logger.error("Failed to process SNS Webhook: %s", str(e))
            log_vals.update({'status': 'failed', 'error_message': str(e)})
            
        finally:
            request.env['ses.webhook.log'].sudo().create(log_vals)

        return request.make_response("OK", status=200)
