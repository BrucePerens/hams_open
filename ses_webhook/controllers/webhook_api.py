# SPDX-License-Identifier: AGPL-3.0-or-later
import email
import email.policy
import logging
import json
import re
import urllib.request
from odoo import http
from odoo.http import request

_logger = logging.getLogger(__name__)

# Adversarial security review, 2026-09-03: real AWS SNS subscription-
# confirmation URLs are always on a sns.<region>.amazonaws.com host over
# HTTPS -- anyone holding a domain's shared webhook token (leaked via a
# proxy/access log, browser history, or a compromised AWS console -- the
# token lives in a plain query-string URL, not a signed AWS message) could
# otherwise supply an arbitrary SubscribeURL and make this server fetch it.
# On an EC2-hosted instance with the metadata service reachable, that's a
# real path to steal the instance's own IAM credentials
# (http://169.254.169.254/...), or to probe/attack other internal-only
# services -- a classic SSRF, not a hypothetical.
_SNS_SUBSCRIBE_URL_RE = re.compile(
    r"^https://sns\.[a-z0-9-]+\.amazonaws\.com/", re.IGNORECASE
)

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
                if subscribe_url and _SNS_SUBSCRIBE_URL_RE.match(subscribe_url):
                    # Adversarial security review, 2026-09-03: real, explicit
                    # timeout -- the old call had none at all, so a
                    # SubscribeURL pointing at a server that accepts the
                    # connection and never responds hung this worker
                    # indefinitely (the same unbounded-hang bug class fixed
                    # in several other daemons this session).
                    urllib.request.urlopen(subscribe_url, timeout=10)
                    _logger.info("Successfully confirmed SNS subscription for domain %s.", domain.name)
                    log_vals.update({'status': 'success'})
                elif subscribe_url:
                    _logger.warning(
                        "SES Webhook: refusing to fetch a SubscribeURL that isn't a real "
                        "AWS SNS host for domain %s: %s", domain.name, subscribe_url,
                    )
                    log_vals.update({'status': 'rejected_subscribe_url'})
                    
            elif payload_type == 'Notification':
                ses_message_str = payload.get('Message', '{}')
                ses_message = json.loads(ses_message_str)

                # SES event notifications (bounce/complaint/delivery -- see
                # docs/proposals/EMAIL_SEND_RECEIVE.md's "Bounce and
                # complaint handling" section) arrive on this same SNS
                # topic shape as inbound mail, distinguished by
                # 'notificationType' instead of a 'content' field. Must be
                # checked first: a Bounce/Complaint payload has no
                # 'content' at all, so falling through to the raw-email
                # branch below would just silently log it as "no content
                # field found" and lose the suppression signal entirely.
                notification_type = ses_message.get('notificationType')
                if notification_type in ('Bounce', 'Complaint'):
                    suppressed = self._handle_ses_event_notification(notification_type, ses_message)
                    _logger.info(
                        "SES %s notification for domain %s: suppressed %d address(es).",
                        notification_type, domain.name, len(suppressed),
                    )
                    log_vals.update({'status': 'success'})
                    return request.make_response("OK", status=200)

                raw_email = ses_message.get('content')

                if not raw_email:
                    _logger.warning("SES Webhook received Notification with no 'content' field for domain %s.", domain.name)
                    log_vals.update({'status': 'ignored', 'error_message': 'No content field found.'})
                else:
                    email_bytes = raw_email.encode('utf-8')

                    # SES_WEBHOOK_SENDER_REGISTRATION.md section 1: detect
                    # the unmatched-sender case explicitly, before ever
                    # calling message_process() -- that method's own sender
                    # resolution (_mail_find_user_for_gateway) does NOT
                    # fail closed on a no-match, it silently falls back to
                    # the caller's own ambient uid and still creates a
                    # record, just misattributed. Parses the same bytes
                    # message_process() is about to parse again internally
                    # (a real, named tradeoff, not an oversight -- see that
                    # doc) using the exact same email.message_from_bytes(...,
                    # policy=email.policy.SMTP) + message_parse() sequence
                    # message_process() itself uses, so this gate's notion
                    # of "the sender" matches message_process()'s own
                    # exactly rather than a second, subtly different parse.
                    #
                    # .sudo() here for the same reason message_process()'s
                    # own .sudo() below is justified: message_parse() reads
                    # mail.alias.domain internally (Odoo 19's per-company
                    # alias-domain lookup), which this route's public/
                    # service-account env has no ACL for, and neither
                    # message_parse() nor _mail_find_user_for_gateway()
                    # writes anything -- _mail_find_user_for_gateway()
                    # already self-elevates internally for its own partner
                    # search regardless of caller privilege (see its own
                    # Odoo source). A read-only parse under .sudo() carries
                    # no more privilege than the .sudo()'d message_process()
                    # call moments later would apply to the exact same
                    # bytes for a matched sender anyway.
                    parsed_message = email.message_from_bytes(email_bytes, policy=email.policy.SMTP)
                    msg_dict = request.env['mail.thread'].sudo().message_parse(parsed_message)  # burn-ignore-sudo: read-only parse, no ACL for mail.alias.domain otherwise; see comment above.
                    email_from = msg_dict.get('email_from')
                    matched_user = (
                        request.env['mail.thread']._mail_find_user_for_gateway(email_from)
                        if email_from else request.env['res.users']
                    )

                    if not matched_user:
                        # Unregistered/unmatched sender: hold the
                        # submission and actively nudge them toward
                        # registration (section 2/3/4), rather than
                        # letting message_process() silently create a
                        # record attributed to nobody in particular.
                        request.env['ses.webhook.pending_submission'].with_user(svc_uid).with_company(
                            domain.company_id
                        ).create_and_notify(
                            sender_email=email_from or 'unknown',
                            raw_content=raw_email,
                            domain=domain,
                        )
                        _logger.info(
                            "SES Webhook: unmatched sender for domain %s -- routed to registration nudge, not message_process.",
                            domain.name,
                        )
                        log_vals.update({
                            'status': 'ignored',
                            'error_message': f'Unregistered sender ({email_from or "unknown"}): routed to registration nudge.',
                        })
                        return request.make_response("OK", status=200)

                    # Matched sender: proceed exactly as before.
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

    def _handle_ses_event_notification(self, notification_type, ses_message):
        """Suppresses future sends to addresses SES reports as bounced/complained, using
        Odoo's own mail.blacklist -- the same suppression list message_process() and every
        outbound send already consult, so no new send-time check is needed anywhere else.

        A spam complaint is always suppressed immediately, regardless of count: a single
        complaint is a real signal a recipient doesn't want this mail, and repeat complaints
        are exactly what AWS SES account-level reputation monitoring escalates on.

        For bounces, only 'Permanent' bounceType is suppressed -- SES's own documented
        distinction: Permanent means the address is invalid/doesn't exist and will never
        succeed, Transient means a temporary condition (mailbox full, temporary block) that
        may well succeed on a later retry, so blacklisting on every transient bounce would
        wrongly and permanently silence a recipient who's still reachable.
        """
        svc_uid = request.env['zero_sudo.security.utils']._get_service_uid(
            'ses_webhook.user_ses_webhook_service_internal'
        )
        blacklist = request.env['mail.blacklist'].with_user(svc_uid)
        suppressed = []

        if notification_type == 'Complaint':
            complaint = ses_message.get('complaint', {})
            for recipient in complaint.get('complainedRecipients', []):
                addr = recipient.get('emailAddress')
                if addr:
                    blacklist._add(addr, message="Suppressed: SES spam complaint (feedbackId %s)." % complaint.get('feedbackId', 'unknown'))
                    suppressed.append(addr)

        elif notification_type == 'Bounce':
            bounce = ses_message.get('bounce', {})
            if bounce.get('bounceType') == 'Permanent':
                for recipient in bounce.get('bouncedRecipients', []):
                    addr = recipient.get('emailAddress')
                    if addr:
                        blacklist._add(addr, message="Suppressed: SES permanent (hard) bounce -- %s" % recipient.get('diagnosticCode', 'no diagnostic code'))
                        suppressed.append(addr)

        return suppressed
