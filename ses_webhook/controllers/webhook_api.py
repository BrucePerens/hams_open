# SPDX-License-Identifier: AGPL-3.0-or-later
import base64
import email
import email.policy
import functools
import logging
import json
import re
import urllib.request
from datetime import datetime, timezone

from cryptography import x509
from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives import hashes
from cryptography.hazmat.primitives.asymmetric import padding

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
_SNS_HOST_PATTERN = r"sns\.[a-z0-9-]+\.amazonaws\.com"
_SNS_SUBSCRIBE_URL_RE = re.compile(
    rf"^https://{_SNS_HOST_PATTERN}/", re.IGNORECASE
)

# Bug-hunt finding, 2026-09-09 (receive_sns_webhook.md, bug class 26): the
# SubscribeURL host check above closed the SSRF sub-vector, but nothing
# verified the request actually came from AWS SNS at all -- the entire
# trust boundary was one opaque per-domain token, leakable via a plain
# query-string URL. Real AWS SNS notifications are signed
# (Signature/SigningCertURL/SignatureVersion, verifiable against an
# Amazon-issued X.509 cert); this closes that gap as a second,
# independent layer on top of the token, not a replacement for it.
#
# AWS's real SigningCertURL is always of the shape
# https://sns.<region>.amazonaws.com/SimpleNotificationService-<hex>.pem --
# stricter than the bare host check above (a signing-cert fetch has no
# legitimate reason to hit any other path on that host), so this is its
# own, separate regex rather than reusing _SNS_SUBSCRIBE_URL_RE.
_SNS_SIGNING_CERT_URL_RE = re.compile(
    rf"^https://{_SNS_HOST_PATTERN}/SimpleNotificationService-[0-9a-f]+\.pem$",
    re.IGNORECASE,
)

# AWS's documented field set/order for the string-to-sign. Verified
# directly against AWS's own open-source reference implementation
# (aws/aws-sdk-java, aws-java-sdk-sns's SignatureChecker.java,
# publishMessageValues()/subscribeMessageValues()/stringToSign() --
# fetched and read 2026-09-09, not recalled from memory): that code
# builds a TreeMap<String,String> (alphabetically sorted by key) of the
# same field names below and joins "key\nvalue\n" for every entry,
# including the last -- i.e. every value gets a trailing newline, with
# no special-cased final field. The alphabetical order TreeMap produces
# for these exact key sets happens to equal the tuples below (confirmed
# by hand: Message < MessageId < Subject < Timestamp < TopicArn < Type;
# Message < MessageId < SubscribeURL < Timestamp < Token < TopicArn <
# Type), so listing them in that order here and joining the same way
# reproduces AWS's own reference algorithm exactly, not merely "an order
# that happens to verify against messages this code itself produces."
# Subject is the one field signed only when actually present on the
# message (parsedMessage.containsKey(SUBJECT) in the same reference
# code) -- a Notification with no Subject omits it from the signed
# string entirely, it is never signed as an empty string. AWS's own SNS
# Developer Guide additionally documents this same field set/order in
# prose (SubscriptionConfirmation/UnsubscribeConfirmation are treated
# identically -- the reference code's own comment: "no difference, for
# now").
_NOTIFICATION_SIGNED_FIELDS = ('Message', 'MessageId', 'Subject', 'Timestamp', 'TopicArn', 'Type')
_SUBSCRIBE_SIGNED_FIELDS = ('Message', 'MessageId', 'SubscribeURL', 'Timestamp', 'Token', 'TopicArn', 'Type')

# The only payload_type values ses.webhook.log's own Selection field
# accepts -- anything else attacker-controlled in the JSON body's "Type"
# must be coerced to 'Unknown' before ever reaching a log create() call.
# Found in passing while adding the signature-rejection log path below:
# the pre-existing code wrote payload.get('Type', 'Unknown') into
# log_vals['payload_type'] completely unclamped, so an attacker-supplied
# Type of anything outside the four known values would make the
# `finally` block's own log create() raise ValueError on an invalid
# Selection value -- the exact "unconstrained attacker string into a
# Selection field" shape test_03b already found and fixed once for
# 'status' (rejected_subscribe_url); this is the same bug class in the
# sibling field, closed the same way.
_KNOWN_PAYLOAD_TYPES = {'Notification', 'SubscriptionConfirmation', 'UnsubscribeConfirmation'}


def _build_string_to_sign(payload):
    """Builds the canonical newline-delimited string AWS SNS itself signs, per the message's own
    Type. Returns None (never raises) if the Type isn't one AWS signs at all, or if a field the
    Type requires is simply absent -- both are treated as "cannot verify," not a shortcut past
    verification, so the caller fails closed either way."""
    payload_type = payload.get('Type')
    if payload_type == 'Notification':
        signed_fields = _NOTIFICATION_SIGNED_FIELDS
    elif payload_type in ('SubscriptionConfirmation', 'UnsubscribeConfirmation'):
        signed_fields = _SUBSCRIBE_SIGNED_FIELDS
    else:
        return None

    parts = []
    for field in signed_fields:
        if field == 'Subject':
            if 'Subject' not in payload:
                continue
        elif field not in payload:
            return None
        parts.append(field)
        parts.append(str(payload[field]))
    return "\n".join(parts) + "\n"


# [@ANCHOR: ses_webhook:COMM_fetch_sns_signing_cert]
@functools.lru_cache(maxsize=16)
def _fetch_sns_signing_cert(signing_cert_url):
    """Fetches and caches (by URL) an AWS SNS signing certificate's raw PEM bytes.

    Deliberately has NO host/SSRF check of its own -- the caller (`_verify_sns_signature`) MUST
    validate `signing_cert_url` against `_SNS_SIGNING_CERT_URL_RE` before ever calling this. Kept
    as its own small function, separate from the SubscribeURL fetch in `receive_sns_webhook`
    (which goes through `urllib.request.urlopen` directly and is asserted on by existing tests),
    specifically so tests can patch this one function in isolation without disturbing that
    unrelated assertion or fighting `lru_cache`'s memoization across unrelated test cases.

    `lru_cache` does not memoize a raised exception -- a transient fetch failure is retried on the
    next call rather than being "stuck" returning a cached error forever.
    """
    # Host/path pre-validated by the caller (_verify_sns_signature, against
    # _SNS_SIGNING_CERT_URL_RE) before this function is ever reached.
    with urllib.request.urlopen(signing_cert_url, timeout=10) as resp:
        return resp.read()


# [@ANCHOR: ses_webhook:COMM_verify_sns_signature]
def _verify_sns_signature(payload):
    """Verifies an AWS SNS message's own `Signature` field against the certificate its
    `SigningCertURL` names, per AWS's documented HTTP(S) signing scheme (SignatureVersion 1 =
    SHA1withRSA, SignatureVersion 2 = SHA256withRSA, both RSASSA-PKCS1-v1_5 -- `padding.PKCS1v15`
    is the `cryptography` library's name for that same padding scheme).

    Fails closed: any missing required field, a `SigningCertURL` that isn't a real
    `sns.<region>.amazonaws.com` signing-cert URL, an unreachable/malformed/expired certificate,
    an unsupported `SignatureVersion`, or an actual signature mismatch all return False. Never
    raises -- callers get a plain boolean gate, identical in shape to the token check this
    supplements.
    """
    try:
        signature_b64 = payload.get('Signature')
        signing_cert_url = payload.get('SigningCertURL')
        signature_version = payload.get('SignatureVersion', '1')

        if not signature_b64 or not signing_cert_url:
            return False

        if not _SNS_SIGNING_CERT_URL_RE.match(signing_cert_url):
            _logger.warning(
                "SES Webhook: refusing to trust a SigningCertURL that isn't a real AWS SNS "
                "signing-cert host: %s", signing_cert_url,
            )
            return False

        string_to_sign = _build_string_to_sign(payload)
        if string_to_sign is None:
            return False

        if signature_version == '1':
            hash_algorithm = hashes.SHA1()
        elif signature_version == '2':
            hash_algorithm = hashes.SHA256()
        else:
            _logger.warning("SES Webhook: unsupported SignatureVersion %r.", signature_version)
            return False

        cert_pem = _fetch_sns_signing_cert(signing_cert_url)
        certificate = x509.load_pem_x509_certificate(cert_pem)

        now = datetime.now(timezone.utc)
        if now < certificate.not_valid_before_utc or now > certificate.not_valid_after_utc:
            _logger.warning(
                "SES Webhook: SigningCertURL's certificate is expired or not yet valid: %s",
                signing_cert_url,
            )
            return False

        public_key = certificate.public_key()
        signature = base64.b64decode(signature_b64, validate=True)

        public_key.verify(signature, string_to_sign.encode('utf-8'), padding.PKCS1v15(), hash_algorithm)
        return True
    except InvalidSignature:
        return False
    except Exception as e:  # audit-ignore-catch-all: fail-closed -- ANY parse/network/crypto error here means "not verified," never "trust it anyway."
        _logger.warning("SES Webhook: signature verification raised %s: %s", type(e).__name__, e)
        return False


class SesWebhookController(http.Controller):
    
    @http.route('/mail/webhook/sns', type='http', auth='public', methods=['POST'], csrf=False)
    def receive_sns_webhook(self, **kwargs):
        # [@ANCHOR: ses_webhook:COMM_receive_sns_webhook]
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

        if not isinstance(payload, dict):
            # A JSON array/string/number is valid JSON but not a valid SNS
            # message shape -- every .get() below assumes a dict. Found in
            # passing while adding signature verification: previously this
            # would 500 instead of 400 (e.g. raw_data == '[]').
            return request.make_response("Invalid JSON", status=400)

        payload_type = payload.get('Type', 'Unknown')
        if payload_type not in _KNOWN_PAYLOAD_TYPES:
            payload_type = 'Unknown'
        message_id = payload.get('MessageId', 'Unknown')

        # 2. Verify the message is actually signed by AWS SNS -- a second,
        # independent layer on top of the per-domain token above (neither
        # replaces the other; see the module-level comment on
        # _verify_sns_signature). Deliberately checked BEFORE any
        # processing/dispatch below, and logged with its own distinct
        # 'rejected_signature' status so a real forgery attempt against a
        # known-valid token is forensically visible, not silently 403'd
        # with no trace the way a plain bad-token request is.
        if not _verify_sns_signature(payload):
            _logger.warning(
                "SES Webhook denied: SNS signature verification failed for domain %s "
                "(Type=%s, MessageId=%s) -- token was valid, but the message itself was "
                "not verifiably signed by AWS SNS.",
                domain.name, payload_type, message_id,
            )
            request.env['ses.webhook.log'].with_user(svc_uid).create({
                'name': message_id,
                'payload_type': payload_type,
                'raw_payload': raw_data,
                'domain_id': domain.id,
                'status': 'rejected_signature',
                'error_message': 'AWS SNS message signature verification failed.',
            })
            return request.make_response("Forbidden", status=403)

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
                # Bug-hunt note, 2026-09-09: this branch is currently
                # unreachable in practice. _verify_sns_signature (called
                # above, before this dispatch) rejects with 403 for any
                # Type outside the three signed types (SignatureChecker
                # can't build a string-to-sign for one), so payload_type
                # can no longer actually be 'Unknown' by the time this
                # code runs. Left in place as defensive dead code rather
                # than removed -- a future change to the signature-gate
                # ordering/scope would silently reopen this path, and it
                # costs nothing to keep a sane fallback for that case.
                # See receive_sns_webhook.md's own claim for the full
                # analysis (bug-hunt Known Bug Class 1).
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

    # [@ANCHOR: ses_webhook:COMM_handle_ses_event_notification]
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
