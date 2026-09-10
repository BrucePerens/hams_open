# SPDX-License-Identifier: AGPL-3.0-or-later
import base64
import json
from datetime import datetime, timedelta, timezone

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import padding, rsa
from cryptography.x509.oid import NameOID

from odoo.addons.zero_sudo.tests.common import HamsHttpCase
# Captured at import time, BEFORE any test's setUp() patches the module
# attribute of the same name -- `from ... import X` binds a direct
# reference to the real, undecorated-by-mock function object, which
# mock.patch('module.X') (patching the *module's* attribute afterward)
# does not retroactively change. Used by the one test
# (test_33_fetch_sns_signing_cert_caches_by_url) that needs to exercise
# the real lru_cache behavior directly, bypassing every other test's
# mock of this same name.
from odoo.addons.ses_webhook.controllers.webhook_api import (
    _fetch_sns_signing_cert as _REAL_FETCH_SNS_SIGNING_CERT,
)

from odoo.exceptions import AccessError
from odoo.tests.common import tagged
from odoo.tools import mute_logger


# A realistic-shaped SigningCertURL: real AWS ones are always
# https://sns.<region>.amazonaws.com/SimpleNotificationService-<32-hex>.pem
# -- matches webhook_api.py's own _SNS_SIGNING_CERT_URL_RE.
_TEST_SIGNING_CERT_URL = (
    "https://sns.us-east-1.amazonaws.com/"
    "SimpleNotificationService-0123456789abcdef0123456789abcdef.pem"
)


def _make_self_signed_cert(private_key, not_before=None, not_after=None):
    """Builds a real, self-signed X.509 certificate for `private_key`, standing in for the
    Amazon-issued cert a real SigningCertURL would serve. Deliberately NOT a mock of the
    verification function itself (that would be a vacuous test per this project's own bug-hunt
    Known Bug Class 2) -- this is a real certificate, real key, real signature, checked by the
    real `cryptography` verification path in webhook_api.py."""
    now = datetime.now(timezone.utc)
    not_before = not_before or (now - timedelta(days=1))
    not_after = not_after or (now + timedelta(days=365))
    name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "sns.amazonaws.com")])
    cert = (
        x509.CertificateBuilder()
        .subject_name(name)
        .issuer_name(name)
        .public_key(private_key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(not_before)
        .not_valid_after(not_after)
        .sign(private_key, hashes.SHA256())
    )
    return cert.public_bytes(serialization.Encoding.PEM)


def _sns_string_to_sign(payload):
    """Independently re-derives AWS SNS's own documented string-to-sign format -- deliberately
    NOT imported from webhook_api.py's own `_build_string_to_sign`. If this test helper and the
    production builder ever disagree, a signature this helper produces must fail against
    production, which is the whole point: importing the production builder to do the signing
    would make the "valid signature verifies" test tautological (it would pass even if both were
    wrong in the same way)."""
    payload_type = payload["Type"]
    if payload_type == "Notification":
        fields = ["Message", "MessageId"]
        if "Subject" in payload:
            fields.append("Subject")
        fields += ["Timestamp", "TopicArn", "Type"]
    elif payload_type in ("SubscriptionConfirmation", "UnsubscribeConfirmation"):
        fields = ["Message", "MessageId", "SubscribeURL", "Timestamp", "Token", "TopicArn", "Type"]
    else:
        raise ValueError(f"Don't know how to sign Type={payload_type!r}")

    parts = []
    for field in fields:
        parts.append(field)
        parts.append(str(payload[field]))
    return "\n".join(parts) + "\n"


def _sign_sns_payload(payload, private_key, signature_version="1", signing_cert_url=_TEST_SIGNING_CERT_URL):
    """Returns a copy of `payload` filled out with real, required-but-previously-absent SNS
    fields (Timestamp/TopicArn/Token as needed) plus a real Signature computed against
    `private_key`, exactly the way AWS SNS itself signs an outgoing notification."""
    payload = dict(payload)
    payload.setdefault("Timestamp", "2026-09-09T00:00:00.000Z")
    payload.setdefault("TopicArn", "arn:aws:sns:us-east-1:123456789012:test-topic")
    # Real AWS SubscriptionConfirmation/UnsubscribeConfirmation messages always carry a
    # human-readable "Message" too (the "You have chosen to subscribe..." text) -- it's a
    # required signed field for every Type, not just Notification (whose own tests already set
    # a real one, the SES event JSON), so default it here rather than in every individual test.
    payload.setdefault("Message", "You have chosen to subscribe to the topic.")
    if payload["Type"] in ("SubscriptionConfirmation", "UnsubscribeConfirmation"):
        payload.setdefault("Token", "test-token-value")

    string_to_sign = _sns_string_to_sign(payload)
    hash_algorithm = hashes.SHA1() if signature_version == "1" else hashes.SHA256()
    signature = private_key.sign(string_to_sign.encode("utf-8"), padding.PKCS1v15(), hash_algorithm)

    payload["Signature"] = base64.b64encode(signature).decode("ascii")
    payload["SignatureVersion"] = signature_version
    payload["SigningCertURL"] = signing_cert_url
    return payload


@tagged('post_install', '-at_install')
class TestSesWebhook(HamsHttpCase):

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        # Real RSA keypair + real self-signed certificate, generated once per test class --
        # every signed test payload below is verified against this real cryptographic material
        # by webhook_api.py's real, unmocked `_verify_sns_signature`. Only the network fetch of
        # "the cert bytes living at SigningCertURL" is mocked (in setUp, below) -- there is no
        # real AWS endpoint to fetch from in a test sandbox, and _SNS_SIGNING_CERT_URL_RE's own
        # SSRF guard means this suite can't legitimately point SigningCertURL anywhere but a real
        # sns.*.amazonaws.com host anyway.
        cls._sns_private_key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
        cls._sns_cert_pem = _make_self_signed_cert(cls._sns_private_key)
        # A second, unrelated keypair/cert -- used to prove a signature made with the WRONG key
        # (as opposed to a bit-flipped signature) is still rejected.
        cls._sns_other_private_key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
        # Four companies, not two: stress-tests the service-account
        # mechanism under more realistic multi-tenant conditions than a
        # single pairwise A/B check can. Company D is deliberately never
        # referenced by any test below except the inclusion test that
        # proves the mechanism doesn't enumerate tenants.
        cls.company_a = cls.env.company
        cls.company_b = cls.env['res.company'].create({'name': 'Company B'})
        cls.company_c = cls.env['res.company'].create({'name': 'Company C'})
        cls.company_d = cls.env['res.company'].create({'name': 'Company D'})

        # Create a mock webhook domain configuration for Company A
        cls.domain_a = cls.env['ses.webhook.domain'].create({
            'name': 'test-a.com',
            'secret_token': 'mock_secret_a',
            'company_id': cls.company_a.id
        })

        # Create a mock webhook domain configuration for Company B
        cls.domain_b = cls.env['ses.webhook.domain'].create({
            'name': 'test-b.com',
            'secret_token': 'mock_secret_b',
            'company_id': cls.company_b.id
        })

        # Company C gets a domain too, so exclusion checks below have a
        # third distinct tenant to confirm invisibility against, not just
        # a single pairwise A/B check. Company D deliberately does NOT get
        # a domain here -- the inclusion test creates it fresh, to prove
        # nothing about this mechanism depends on setup-time enumeration.
        cls.domain_c = cls.env['ses.webhook.domain'].create({
            'name': 'test-c.com',
            'secret_token': 'mock_secret_c',
            'company_id': cls.company_c.id
        })

        # SES_WEBHOOK_SENDER_REGISTRATION.md's registration gate: several
        # tests below send a Notification "From:" a sender address and
        # expect it to reach message_process() -- that now requires a real
        # registered user matching that email (_mail_find_user_for_gateway),
        # not just an arbitrary synthetic address. These two are the
        # "already registered" senders those tests exercise.
        cls.matched_user_a = cls.env['res.users'].create({
            'name': 'Matched Sender A',
            'login': 'matched_sender_a',
            'email': 'a@test-a.com',
            'group_ids': [(6, 0, [cls.env.ref('base.group_portal').id])],
        })
        cls.matched_user_b = cls.env['res.users'].create({
            'name': 'Matched Sender B',
            'login': 'matched_sender_b',
            'email': 'b@test-b.com',
            'group_ids': [(6, 0, [cls.env.ref('base.group_portal').id])],
        })

    def setUp(self):
        super().setUp()
        # Every test below now needs *some* AWS SNS signature-verification behavior, since
        # webhook_api.py's own _verify_sns_signature runs before any payload dispatch. Rather
        # than making a real HTTPS request to a real sns.*.amazonaws.com host (there is none in
        # this sandbox, and _SNS_SIGNING_CERT_URL_RE's own SSRF guard means there's nowhere else
        # legitimate to point it), the cert *fetch* -- not the verification math itself -- is
        # mocked here, patched fresh per test so each test can override its return_value/
        # side_effect independently. This is the one function boundary the production code was
        # deliberately split at for exactly this reason -- see _fetch_sns_signing_cert's own
        # docstring in webhook_api.py.
        self.cert_fetch_mock = self.safe_patch(
            'odoo.addons.ses_webhook.controllers.webhook_api._fetch_sns_signing_cert'
        )
        self.cert_fetch_mock.return_value = self._sns_cert_pem

    def _sign(self, payload, **kwargs):
        """Shorthand for signing a test payload with this class's own real test keypair."""
        return _sign_sns_payload(payload, self._sns_private_key, **kwargs)

    def test_01_webhook_unauthorized(self):
        # Tests [@ANCHOR: ses_webhook:COMM_receive_sns_webhook]
        """Verify that requests without the correct token are rejected with 403 Forbidden."""
        response = self.url_open('/mail/webhook/sns', data=b'{}', headers={'Content-Type': 'application/json'})
        self.assertEqual(response.status_code, 403, "Should reject without token")

        response = self.url_open('/mail/webhook/sns?token=wrong_token', data=b'{}', headers={'Content-Type': 'application/json'})
        self.assertEqual(response.status_code, 403, "Should reject with wrong token")

    def test_02_webhook_empty_or_invalid_payload(self):
        """Verify handling of empty or invalid JSON payloads."""
        response = self.url_open(f'/mail/webhook/sns?token={self.domain_a.secret_token}', data=b' ', headers={'Content-Type': 'application/json'})
        self.assertEqual(response.status_code, 400, "Should reject empty payload")
        
        response = self.url_open(f'/mail/webhook/sns?token={self.domain_a.secret_token}', data=b'not json', headers={'Content-Type': 'text/plain'})
        self.assertEqual(response.status_code, 400, "Should reject invalid JSON")

    def test_03_webhook_subscription_confirmation(self):
        """Verify SubscriptionConfirmation visits the SubscribeURL and logs correctly."""
        payload = {
            "Type": "SubscriptionConfirmation",
            "MessageId": "msg-sub-1",
            # A real AWS SNS-shaped host: the SSRF-hardening regex added
            # 2026-09-03 (_SNS_SUBSCRIBE_URL_RE) only fetches URLs matching
            # sns.<region>.amazonaws.com over HTTPS -- this test's own
            # mock URL used to be a plain http://mock-aws.com host, which
            # no longer matches, so it was silently exercising the
            # rejected-URL branch instead of the success path it was
            # actually written to test.
            "SubscribeURL": "https://sns.us-east-1.amazonaws.com/confirm"
        }
        payload = self._sign(payload)
        mock_urlopen = self.safe_patch('urllib.request.urlopen')
        mock_urlopen.return_value = True
        response = self.url_open(f'/mail/webhook/sns?token={self.domain_a.secret_token}', data=json.dumps(payload).encode('utf-8'))
        self.assertEqual(response.status_code, 200)
        mock_urlopen.assert_called_once_with(
            "https://sns.us-east-1.amazonaws.com/confirm", timeout=10
        )

        log = self.env['ses.webhook.log'].search([('name', '=', 'msg-sub-1')])
        self.assertEqual(len(log), 1)
        self.assertEqual(log.status, 'success')
        self.assertEqual(log.domain_id, self.domain_a)

    def test_03b_webhook_subscription_confirmation_rejects_non_aws_url(self):
        """A real, previously-undiscovered production bug: the reject
        branch set status='rejected_subscribe_url', a value never added
        to ses.webhook.log's own Selection field, so ANY non-AWS
        SubscribeURL (exactly the SSRF scenario this regex exists to
        defend against) crashed the whole webhook handler with an
        uncaught 500 instead of logging the rejection gracefully. Found
        live by this test suite's own real run, not by static review."""
        payload = {
            "Type": "SubscriptionConfirmation",
            "MessageId": "msg-sub-rejected",
            "SubscribeURL": "https://evil.example.com/confirm",
        }
        # Signed validly (over the literal evil URL, exactly as AWS would
        # sign whatever SubscribeURL a real Subscribe API call produced) --
        # this test is about the SubscribeURL host check specifically, not
        # signature verification, so the signature itself must pass to
        # reach that check at all.
        payload = self._sign(payload)
        mock_urlopen = self.safe_patch('urllib.request.urlopen')
        response = self.url_open(
            f'/mail/webhook/sns?token={self.domain_a.secret_token}',
            data=json.dumps(payload).encode('utf-8'),
        )
        self.assertEqual(response.status_code, 200)
        mock_urlopen.assert_not_called()

        log = self.env['ses.webhook.log'].search([('name', '=', 'msg-sub-rejected')])
        self.assertEqual(len(log), 1)
        self.assertEqual(log.status, 'rejected_subscribe_url')

    def test_04_webhook_notification_processed_company_a(self):
        """Verify Notification extracts content and passes to mail.thread in Company A context."""
        raw_email = b"From: a@test-a.com\nTo: c@d.com\nSubject: Test A\n\nTest"
        ses_message = {"notificationType": "Received", "content": raw_email.decode('utf-8')}
        payload = {"Type": "Notification", "MessageId": "msg-notif-a", "Message": json.dumps(ses_message)}
        payload = self._sign(payload)

        mock_process = self.safe_patch('odoo.addons.mail.models.mail_thread.MailThread.message_process')
        mock_process.return_value = True
        response = self.url_open(f'/mail/webhook/sns?token={self.domain_a.secret_token}', data=json.dumps(payload).encode('utf-8'))
        self.assertEqual(response.status_code, 200)

        mock_process.assert_called_once()
        args, kwargs = mock_process.call_args
        self.assertEqual(args[1], raw_email)

        # Since message_process was called on a recordset with `with_company`, we check the env of the mocked call
        # But we can just verify the log is assigned correctly
        log = self.env['ses.webhook.log'].search([('name', '=', 'msg-notif-a')])
        self.assertEqual(len(log), 1)
        self.assertEqual(log.status, 'success')
        self.assertEqual(log.domain_id, self.domain_a)
        self.assertEqual(log.company_id, self.company_a)

    def test_05_webhook_notification_processed_company_b(self):
        """Verify Notification extracts content and passes to mail.thread in Company B context."""
        raw_email = b"From: b@test-b.com\nTo: c@d.com\nSubject: Test B\n\nTest"
        ses_message = {"notificationType": "Received", "content": raw_email.decode('utf-8')}
        payload = {"Type": "Notification", "MessageId": "msg-notif-b", "Message": json.dumps(ses_message)}
        payload = self._sign(payload)

        mock_process = self.safe_patch('odoo.addons.mail.models.mail_thread.MailThread.message_process')
        mock_process.return_value = True
        response = self.url_open(f'/mail/webhook/sns?token={self.domain_b.secret_token}', data=json.dumps(payload).encode('utf-8'))
        self.assertEqual(response.status_code, 200)

        mock_process.assert_called_once()
        args, kwargs = mock_process.call_args
        self.assertEqual(args[1], raw_email)

        log = self.env['ses.webhook.log'].search([('name', '=', 'msg-notif-b')])
        self.assertEqual(len(log), 1)
        self.assertEqual(log.status, 'success')
        self.assertEqual(log.domain_id, self.domain_b)
        self.assertEqual(log.company_id, self.company_b)

    def test_06_webhook_notification_no_content(self):
        """Verify Notification without 'content' logs an error and ignores it."""
        ses_message = {"notificationType": "Received"}
        payload = {"Type": "Notification", "MessageId": "msg-no-content", "Message": json.dumps(ses_message)}
        payload = self._sign(payload)

        response = self.url_open(f'/mail/webhook/sns?token={self.domain_a.secret_token}', data=json.dumps(payload).encode('utf-8'))
        self.assertEqual(response.status_code, 200) # Returns 200 to AWS to stop retries
        
        log = self.env['ses.webhook.log'].search([('name', '=', 'msg-no-content')])
        self.assertEqual(len(log), 1)
        self.assertEqual(log.status, 'ignored')
        self.assertIn('No content field found', log.error_message)

    def test_07_webhook_unsubscribe_confirmation(self):
        """Verify UnsubscribeConfirmation is ignored properly."""
        payload = {
            "Type": "UnsubscribeConfirmation",
            "MessageId": "msg-unsub",
            # Real UnsubscribeConfirmation messages carry a SubscribeURL
            # (to resubscribe) the same way SubscriptionConfirmation does
            # -- it's one of the fields AWS itself signs for this Type.
            "SubscribeURL": "https://sns.us-east-1.amazonaws.com/resubscribe",
        }
        payload = self._sign(payload)

        response = self.url_open(f'/mail/webhook/sns?token={self.domain_a.secret_token}', data=json.dumps(payload).encode('utf-8'))
        self.assertEqual(response.status_code, 200)
        
        log = self.env['ses.webhook.log'].search([('name', '=', 'msg-unsub')])
        self.assertEqual(len(log), 1)
        self.assertEqual(log.status, 'ignored')

    def test_23_webhook_complaint_blacklists_immediately(self):
        # Tests [@ANCHOR: ses_webhook:COMM_handle_ses_event_notification]
        """A spam complaint suppresses the complained recipient outright, regardless of count --
        the real gap this AWS SES production-access review surfaced: complaints don't arrive as
        a bounce DSN email, they need their own SNS notificationType handler."""
        ses_message = {
            "notificationType": "Complaint",
            "complaint": {
                "complainedRecipients": [{"emailAddress": "complainer@example.com"}],
                "feedbackId": "feedback-1",
            },
            "mail": {"messageId": "mail-1"},
        }
        payload = {"Type": "Notification", "MessageId": "msg-complaint-1", "Message": json.dumps(ses_message)}
        payload = self._sign(payload)

        response = self.url_open(f'/mail/webhook/sns?token={self.domain_a.secret_token}', data=json.dumps(payload).encode('utf-8'))
        self.assertEqual(response.status_code, 200)

        blacklisted = self.env['mail.blacklist'].search([('email', '=', 'complainer@example.com')])
        self.assertEqual(len(blacklisted), 1)

        log = self.env['ses.webhook.log'].search([('name', '=', 'msg-complaint-1')])
        self.assertEqual(log.status, 'success')

    def test_24_webhook_bounce_permanent_blacklists_transient_does_not(self):
        """A Permanent (hard) bounce suppresses the address; a Transient (soft) bounce must not --
        blacklisting on every transient bounce would wrongly, permanently silence a recipient
        who may well still be reachable on a later attempt."""
        permanent_message = {
            "notificationType": "Bounce",
            "bounce": {
                "bounceType": "Permanent",
                "bouncedRecipients": [{"emailAddress": "harddown@example.com", "diagnosticCode": "550 5.1.1 no such user"}],
                "feedbackId": "feedback-2",
            },
        }
        payload = {"Type": "Notification", "MessageId": "msg-bounce-permanent", "Message": json.dumps(permanent_message)}
        payload = self._sign(payload)
        response = self.url_open(f'/mail/webhook/sns?token={self.domain_a.secret_token}', data=json.dumps(payload).encode('utf-8'))
        self.assertEqual(response.status_code, 200)
        self.assertEqual(len(self.env['mail.blacklist'].search([('email', '=', 'harddown@example.com')])), 1)

        transient_message = {
            "notificationType": "Bounce",
            "bounce": {
                "bounceType": "Transient",
                "bouncedRecipients": [{"emailAddress": "mailboxfull@example.com", "diagnosticCode": "452 4.2.2 mailbox full"}],
                "feedbackId": "feedback-3",
            },
        }
        payload = {"Type": "Notification", "MessageId": "msg-bounce-transient", "Message": json.dumps(transient_message)}
        payload = self._sign(payload)
        response = self.url_open(f'/mail/webhook/sns?token={self.domain_a.secret_token}', data=json.dumps(payload).encode('utf-8'))
        self.assertEqual(response.status_code, 200)
        self.assertEqual(len(self.env['mail.blacklist'].search([('email', '=', 'mailboxfull@example.com')])), 0)

    # ------------------------------------------------------------------
    # AWS SNS message-signature verification (bug-hunt fix, 2026-09-09):
    # a leaked per-domain token alone must no longer be sufficient to
    # forge a notification -- see receive_sns_webhook.md's claim and
    # night_shift_todo.md's own entry for the full finding this closes.
    # ------------------------------------------------------------------

    def test_25_signature_valid_notification_accepted(self):
        # Tests [@ANCHOR: ses_webhook:COMM_verify_sns_signature]
        """A validly-signed Notification (SignatureVersion 1, the AWS default) is accepted and
        processed exactly as before -- the baseline positive case every other rejection test
        below is contrasted against."""
        raw_email = b"From: a@test-a.com\nTo: c@d.com\nSubject: Signed\n\nTest"
        ses_message = {"notificationType": "Received", "content": raw_email.decode('utf-8')}
        payload = {"Type": "Notification", "MessageId": "msg-sig-valid-v1", "Message": json.dumps(ses_message)}
        payload = self._sign(payload, signature_version="1")

        mock_process = self.safe_patch('odoo.addons.mail.models.mail_thread.MailThread.message_process')
        mock_process.return_value = True
        response = self.url_open(f'/mail/webhook/sns?token={self.domain_a.secret_token}', data=json.dumps(payload).encode('utf-8'))
        self.assertEqual(response.status_code, 200)
        mock_process.assert_called_once()

        log = self.env['ses.webhook.log'].search([('name', '=', 'msg-sig-valid-v1')])
        self.assertEqual(log.status, 'success')

    def test_25b_signature_with_subject_field_accepted(self):
        # Tests [@ANCHOR: ses_webhook:COMM_verify_sns_signature]
        """AWS signs 'Subject' only when the message actually carries one, in a specific position
        in the signed field order (between MessageId and Timestamp) -- a real Notification with a
        Subject must still verify correctly, not just the more common Subject-less case every
        other test here exercises."""
        raw_email = b"From: a@test-a.com\nTo: c@d.com\nSubject: Has A Subject\n\nTest"
        ses_message = {"notificationType": "Received", "content": raw_email.decode('utf-8')}
        payload = {
            "Type": "Notification",
            "MessageId": "msg-sig-with-subject",
            "Subject": "SES Notification",
            "Message": json.dumps(ses_message),
        }
        payload = self._sign(payload)

        mock_process = self.safe_patch('odoo.addons.mail.models.mail_thread.MailThread.message_process')
        mock_process.return_value = True
        response = self.url_open(f'/mail/webhook/sns?token={self.domain_a.secret_token}', data=json.dumps(payload).encode('utf-8'))
        self.assertEqual(response.status_code, 200)
        mock_process.assert_called_once()

        log = self.env['ses.webhook.log'].search([('name', '=', 'msg-sig-with-subject')])
        self.assertEqual(log.status, 'success')

    def test_26_signature_version_2_sha256_accepted(self):
        # Tests [@ANCHOR: ses_webhook:COMM_verify_sns_signature]
        """SignatureVersion 2 (SHA256, AWS's recommended stronger option) is a fully supported,
        independently-exercised code path, not just version 1."""
        raw_email = b"From: a@test-a.com\nTo: c@d.com\nSubject: Signed V2\n\nTest"
        ses_message = {"notificationType": "Received", "content": raw_email.decode('utf-8')}
        payload = {"Type": "Notification", "MessageId": "msg-sig-valid-v2", "Message": json.dumps(ses_message)}
        payload = self._sign(payload, signature_version="2")

        mock_process = self.safe_patch('odoo.addons.mail.models.mail_thread.MailThread.message_process')
        mock_process.return_value = True
        response = self.url_open(f'/mail/webhook/sns?token={self.domain_a.secret_token}', data=json.dumps(payload).encode('utf-8'))
        self.assertEqual(response.status_code, 200)
        mock_process.assert_called_once()

        log = self.env['ses.webhook.log'].search([('name', '=', 'msg-sig-valid-v2')])
        self.assertEqual(log.status, 'success')

    def test_27_signature_missing_rejected(self):
        # Tests [@ANCHOR: ses_webhook:COMM_verify_sns_signature]
        """The exact shape every OTHER test in this file used to send before this fix, and the
        exact shape a leaked-token attacker would send with no AWS involvement at all: a
        completely unsigned payload. Must now be rejected with 403 and a distinct
        'rejected_signature' log status, even though the token itself is valid."""
        raw_email = b"From: a@test-a.com\nTo: c@d.com\nSubject: Forged\n\nTest"
        ses_message = {"notificationType": "Received", "content": raw_email.decode('utf-8')}
        payload = {"Type": "Notification", "MessageId": "msg-sig-missing", "Message": json.dumps(ses_message)}
        # No Signature/SigningCertURL/SignatureVersion at all -- not signed.

        mock_process = self.safe_patch('odoo.addons.mail.models.mail_thread.MailThread.message_process')
        response = self.url_open(f'/mail/webhook/sns?token={self.domain_a.secret_token}', data=json.dumps(payload).encode('utf-8'))
        self.assertEqual(response.status_code, 403)
        mock_process.assert_not_called()

        log = self.env['ses.webhook.log'].search([('name', '=', 'msg-sig-missing')])
        self.assertEqual(len(log), 1)
        self.assertEqual(log.status, 'rejected_signature')
        self.assertEqual(log.domain_id, self.domain_a)

    def test_28_signature_tampered_rejected(self):
        # Tests [@ANCHOR: ses_webhook:COMM_verify_sns_signature]
        """A validly-structured, validly-signed payload whose Message is altered AFTER signing
        (the classic tamper scenario -- an attacker who captured or partially controls a real
        signed message and modifies its content) must be rejected: the signature no longer
        matches the string it was computed over."""
        ses_message = {"notificationType": "Received", "content": "From: a@test-a.com\nTo: c@d.com\n\nOriginal"}
        payload = {"Type": "Notification", "MessageId": "msg-sig-tampered", "Message": json.dumps(ses_message)}
        payload = self._sign(payload)
        # Tamper with Message AFTER signing -- the Signature field is now stale/invalid for
        # this (different) Message content.
        tampered_message = {"notificationType": "Received", "content": "From: attacker@evil.com\nTo: c@d.com\n\nTampered"}
        payload["Message"] = json.dumps(tampered_message)

        mock_process = self.safe_patch('odoo.addons.mail.models.mail_thread.MailThread.message_process')
        response = self.url_open(f'/mail/webhook/sns?token={self.domain_a.secret_token}', data=json.dumps(payload).encode('utf-8'))
        self.assertEqual(response.status_code, 403)
        mock_process.assert_not_called()

        log = self.env['ses.webhook.log'].search([('name', '=', 'msg-sig-tampered')])
        self.assertEqual(log.status, 'rejected_signature')

    def test_29_signature_wrong_key_rejected(self):
        # Tests [@ANCHOR: ses_webhook:COMM_verify_sns_signature]
        """A signature that is well-formed (right length, valid base64, produced by a REAL RSA
        private key over the REAL correct string-to-sign) but from a key that doesn't match the
        certificate served at SigningCertURL must still be rejected -- distinct from tampering:
        the string-to-sign is untouched, only the signing key differs, which real AWS's
        certificate-bound verification is specifically designed to catch."""
        ses_message = {"notificationType": "Received", "content": "From: a@test-a.com\nTo: c@d.com\n\nX"}
        payload = {"Type": "Notification", "MessageId": "msg-sig-wrongkey", "Message": json.dumps(ses_message)}
        payload = _sign_sns_payload(payload, self._sns_other_private_key)  # NOT self._sns_private_key
        # cert_fetch_mock (from setUp) still serves self._sns_cert_pem, which is the cert for
        # self._sns_private_key -- so this signature won't verify against it.

        mock_process = self.safe_patch('odoo.addons.mail.models.mail_thread.MailThread.message_process')
        response = self.url_open(f'/mail/webhook/sns?token={self.domain_a.secret_token}', data=json.dumps(payload).encode('utf-8'))
        self.assertEqual(response.status_code, 403)
        mock_process.assert_not_called()

        log = self.env['ses.webhook.log'].search([('name', '=', 'msg-sig-wrongkey')])
        self.assertEqual(log.status, 'rejected_signature')

    def test_30_signature_non_aws_cert_host_rejected(self):
        # Tests [@ANCHOR: ses_webhook:COMM_verify_sns_signature]
        """A SigningCertURL pointing anywhere other than a real sns.<region>.amazonaws.com host
        must be rejected WITHOUT ever fetching it -- the same SSRF-prevention shape as the
        existing SubscribeURL host check, applied to the cert fetch. Asserts the cert-fetch mock
        itself is never called, proving the host check runs before any network access, not just
        that the end result happens to be a rejection."""
        ses_message = {"notificationType": "Received", "content": "From: a@test-a.com\nTo: c@d.com\n\nX"}
        payload = {"Type": "Notification", "MessageId": "msg-sig-evilcert", "Message": json.dumps(ses_message)}
        payload = self._sign(payload, signing_cert_url="https://evil.example.com/SimpleNotificationService-abc.pem")

        mock_process = self.safe_patch('odoo.addons.mail.models.mail_thread.MailThread.message_process')
        response = self.url_open(f'/mail/webhook/sns?token={self.domain_a.secret_token}', data=json.dumps(payload).encode('utf-8'))
        self.assertEqual(response.status_code, 403)
        mock_process.assert_not_called()
        self.cert_fetch_mock.assert_not_called()

        log = self.env['ses.webhook.log'].search([('name', '=', 'msg-sig-evilcert')])
        self.assertEqual(log.status, 'rejected_signature')

    def test_30b_signature_cert_url_wrong_path_shape_rejected(self):
        # Tests [@ANCHOR: ses_webhook:COMM_verify_sns_signature]
        """A SigningCertURL on a genuinely correct AWS SNS host, but NOT matching the real
        SimpleNotificationService-<hex>.pem path AWS actually serves certs at, must also be
        rejected -- the host-only check alone isn't the full guard; the path shape matters too
        (a same-host path a real SNS cert endpoint would never actually use)."""
        ses_message = {"notificationType": "Received", "content": "From: a@test-a.com\nTo: c@d.com\n\nX"}
        payload = {"Type": "Notification", "MessageId": "msg-sig-badpath", "Message": json.dumps(ses_message)}
        payload = self._sign(
            payload, signing_cert_url="https://sns.us-east-1.amazonaws.com/not-a-real-cert-path.pem"
        )

        response = self.url_open(f'/mail/webhook/sns?token={self.domain_a.secret_token}', data=json.dumps(payload).encode('utf-8'))
        self.assertEqual(response.status_code, 403)
        self.cert_fetch_mock.assert_not_called()

        log = self.env['ses.webhook.log'].search([('name', '=', 'msg-sig-badpath')])
        self.assertEqual(log.status, 'rejected_signature')

    def test_31_signature_expired_cert_rejected(self):
        # Tests [@ANCHOR: ses_webhook:COMM_verify_sns_signature]
        """A real, validly-signed message whose signing certificate has already expired must be
        rejected -- an expired cert is no longer a trustworthy statement that the key it names
        actually belongs to Amazon SNS. Uses a real expired X.509 certificate (not a mocked
        expiry check), issued for the SAME key that actually signs this payload, so the
        signature math itself is correct and only the cert's own validity window is the reason
        for rejection."""
        expired_cert_pem = _make_self_signed_cert(
            self._sns_private_key,
            not_before=datetime.now(timezone.utc) - timedelta(days=30),
            not_after=datetime.now(timezone.utc) - timedelta(days=1),
        )
        self.cert_fetch_mock.return_value = expired_cert_pem

        ses_message = {"notificationType": "Received", "content": "From: a@test-a.com\nTo: c@d.com\n\nX"}
        payload = {"Type": "Notification", "MessageId": "msg-sig-expired", "Message": json.dumps(ses_message)}
        payload = self._sign(payload)

        response = self.url_open(f'/mail/webhook/sns?token={self.domain_a.secret_token}', data=json.dumps(payload).encode('utf-8'))
        self.assertEqual(response.status_code, 403)

        log = self.env['ses.webhook.log'].search([('name', '=', 'msg-sig-expired')])
        self.assertEqual(log.status, 'rejected_signature')

    def test_32_signature_unsupported_version_rejected(self):
        # Tests [@ANCHOR: ses_webhook:COMM_verify_sns_signature]
        """An unrecognized SignatureVersion (neither '1' nor '2') must fail closed rather than
        falling back to any default algorithm -- checked before ever fetching the certificate."""
        ses_message = {"notificationType": "Received", "content": "From: a@test-a.com\nTo: c@d.com\n\nX"}
        payload = {"Type": "Notification", "MessageId": "msg-sig-badversion", "Message": json.dumps(ses_message)}
        payload = self._sign(payload, signature_version="1")
        payload["SignatureVersion"] = "3"  # Not a real AWS SignatureVersion.

        response = self.url_open(f'/mail/webhook/sns?token={self.domain_a.secret_token}', data=json.dumps(payload).encode('utf-8'))
        self.assertEqual(response.status_code, 403)
        self.cert_fetch_mock.assert_not_called()

        log = self.env['ses.webhook.log'].search([('name', '=', 'msg-sig-badversion')])
        self.assertEqual(log.status, 'rejected_signature')

    def test_33_fetch_sns_signing_cert_caches_by_url(self):
        # Tests [@ANCHOR: ses_webhook:COMM_fetch_sns_signing_cert]
        """Direct, isolated unit test of _fetch_sns_signing_cert itself (bypassing every other
        test's own mock of it, via the reference captured at module-import time): two calls with
        the same URL must issue exactly one real HTTP(S) fetch, the second served from cache."""
        _REAL_FETCH_SNS_SIGNING_CERT.cache_clear()
        mock_urlopen = self.safe_patch('urllib.request.urlopen')
        # `urllib.request.urlopen(...)` is used as `with urlopen(...) as resp: resp.read()` --
        # MagicMock's built-in context-manager support means `.return_value.__enter__` is
        # already a real, callable magic method; only its own return needs configuring.
        mock_urlopen.return_value.__enter__.return_value.read.return_value = b'fake-cert-bytes'

        url = "https://sns.us-east-1.amazonaws.com/SimpleNotificationService-cachetest0000000000000000000000.pem"
        first = _REAL_FETCH_SNS_SIGNING_CERT(url)
        second = _REAL_FETCH_SNS_SIGNING_CERT(url)

        self.assertEqual(first, b'fake-cert-bytes')
        self.assertEqual(second, b'fake-cert-bytes')
        mock_urlopen.assert_called_once_with(url, timeout=10)
        _REAL_FETCH_SNS_SIGNING_CERT.cache_clear()

    def test_34_forged_complaint_without_signature_does_not_blacklist(self):
        # Tests [@ANCHOR: ses_webhook:COMM_verify_sns_signature]

        # Tests [@ANCHOR: ses_webhook:COMM_handle_ses_event_notification]
        """The actual finding this fix closes, end to end: before signature verification, anyone
        holding domain_a's token alone could forge a Complaint naming an arbitrary address and
        get it blacklisted (see test_23's identical payload shape, which used to succeed with no
        signature at all). Now it must be rejected before ever reaching
        _handle_ses_event_notification, and the target address must NOT end up blacklisted."""
        ses_message = {
            "notificationType": "Complaint",
            "complaint": {
                "complainedRecipients": [{"emailAddress": "innocent-target@example.com"}],
                "feedbackId": "forged-feedback",
            },
        }
        payload = {"Type": "Notification", "MessageId": "msg-forged-complaint", "Message": json.dumps(ses_message)}
        # Deliberately unsigned -- exactly what a leaked-token-only attacker can produce.

        response = self.url_open(f'/mail/webhook/sns?token={self.domain_a.secret_token}', data=json.dumps(payload).encode('utf-8'))
        self.assertEqual(response.status_code, 403)

        self.assertFalse(
            self.env['mail.blacklist'].search([('email', '=', 'innocent-target@example.com')]),
            "An unsigned, forged Complaint must NOT be able to blacklist an arbitrary address "
            "just because the per-domain token was valid.",
        )
        log = self.env['ses.webhook.log'].search([('name', '=', 'msg-forged-complaint')])
        self.assertEqual(log.status, 'rejected_signature')

    def test_35_forged_unmatched_sender_notification_does_not_send_nudge(self):
        # Tests [@ANCHOR: ses_webhook:COMM_verify_sns_signature]

        # Tests [@ANCHOR: ses_webhook:COMM_create_and_notify]
        """The second concrete finding this fix closes: before signature verification, anyone
        holding domain_a's token could forge a raw-MIME Notification naming an arbitrary,
        unregistered 'From:' address and trigger a real outbound registration-nudge email TO
        that address FROM the tenant's own domain (see test_19's identical scenario, which used
        to succeed unsigned). Now it must be rejected before create_and_notify ever runs, with
        no pending_submission and no outbound mail.mail created for the forged address."""
        raw_email = b"From: attacker-chosen@example.com\nTo: c@d.com\nSubject: Forged\n\nX"
        ses_message = {"notificationType": "Received", "content": raw_email.decode('utf-8')}
        payload = {"Type": "Notification", "MessageId": "msg-forged-nudge", "Message": json.dumps(ses_message)}
        # Deliberately unsigned.

        mock_process = self.safe_patch('odoo.addons.mail.models.mail_thread.MailThread.message_process')
        response = self.url_open(f'/mail/webhook/sns?token={self.domain_a.secret_token}', data=json.dumps(payload).encode('utf-8'))
        self.assertEqual(response.status_code, 403)
        mock_process.assert_not_called()

        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "ses_webhook.user_ses_webhook_service_internal"
        )
        submission = self.env['ses.webhook.pending_submission'].with_user(svc_uid).search(
            [('sender_email', '=', 'attacker-chosen@example.com')]
        )
        self.assertFalse(
            submission,
            "An unsigned, forged Notification must NOT be able to trigger a real registration-"
            "nudge email to an attacker-chosen address just because the per-domain token was valid.",
        )
        mail = self.env['mail.mail'].search([('email_to', '=', 'attacker-chosen@example.com')])
        self.assertFalse(mail)

        log = self.env['ses.webhook.log'].search([('name', '=', 'msg-forged-nudge')])
        self.assertEqual(log.status, 'rejected_signature')

    def test_09_webhook_url_computes_for_plain_internal_user(self):
        # Tests [@ANCHOR: ses_webhook:COMM_compute_webhook_url]
        """
        _compute_webhook_url() used to read web.base.url via .sudo(),
        forbidden on this platform. ir.config_parameter's only ACL grants
        base.group_system, not base.group_user -- but this model's own ACL
        (access_ses_webhook_domain_user) grants plain base.group_user
        read-only access to ses.webhook.domain, so any such user viewing a
        record needs this compute to still succeed. Fixed to read
        web.base.url via zero_sudo's vetted _get_system_param() instead of
        .sudo(). Prove it actually works for a non-admin viewer, not just
        that .sudo() is gone from the source.

        Uses the canonical odoo_facility_service_internal account rather
        than minting a fresh base.group_user fixture: this test doesn't
        need company-specific scoping (unlike test_10 below), just "a
        plain internal, non-admin viewer" -- which that account already
        is, and check_burn_list.py's DOMAIN SANDBOX audit specifically
        wants base.group_user routed through it instead of scattered
        ad hoc grants where there's no real need for a fresh persona.
        """
        plain_user = self.env.ref("zero_sudo.odoo_facility_service_internal")
        self.assertFalse(plain_user.has_group("base.group_system"))
        self.assertTrue(plain_user.has_group("base.group_user"))

        domain_as_plain_user = self.domain_a.with_user(plain_user)
        self.assertTrue(
            domain_as_plain_user.webhook_url,
            "A plain internal user (base.group_user) MUST be able to "
            "compute/read webhook_url on a record their own ACL grants "
            "them read access to.",
        )

    # ------------------------------------------------------------------
    # EXCLUSION: a persona provably cannot see what isn't theirs.
    # ------------------------------------------------------------------

    def test_10_domain_multi_company_isolation(self):
        """
        ses_webhook_domain_comp_rule is scoped to base.group_user (was a
        GLOBAL rule, no groups field, before the service-account fix --
        globally-scoped ir.rules are unconditionally banned by this repo's
        own linter). Its domain_force is unchanged
        ('|', company_id=False, company_id in company_ids), so every real
        internal employee gets identical isolation to before. Nothing had
        ever proven it actually isolates two companies from each other.
        """
        user_a = self.env["res.users"].create({
            "name": "SES Webhook User A",
            "login": "ses_webhook_user_a",
            "company_id": self.company_a.id,
            "company_ids": [(6, 0, [self.company_a.id])],
            "group_ids": [(6, 0, [self.env.ref("base.group_user").id])],
        })

        seen_by_a = (
            self.env["ses.webhook.domain"].with_user(user_a).search([])
        )
        self.assertIn(self.domain_a, seen_by_a)
        self.assertNotIn(
            self.domain_b,
            seen_by_a,
            "[!] DIAGNOSTIC FOR AI: A plain internal user scoped to "
            "Company A MUST NOT see Company B's ses.webhook.domain "
            "record, even though the model-level ACL alone would "
            "otherwise allow it.",
        )
        self.assertNotIn(self.domain_c, seen_by_a)
        with self.assertRaises(AccessError):
            self.domain_b.with_user(user_a).read(["name"])
        with self.assertRaises(AccessError):
            self.domain_c.with_user(user_a).read(["name"])

    def test_13_domain_multi_company_isolation_symmetric(self):
        """
        The mirror of test_10 with Company A/B swapped -- only one
        direction was ever tested before. A plain base.group_user member
        scoped to Company B must see Company B's own domain and nothing
        from Company A or Company C.
        """
        user_b = self.env["res.users"].create({
            "name": "SES Webhook User B",
            "login": "ses_webhook_user_b",
            "company_id": self.company_b.id,
            "company_ids": [(6, 0, [self.company_b.id])],
            "group_ids": [(6, 0, [self.env.ref("base.group_user").id])],
        })

        seen_by_b = (
            self.env["ses.webhook.domain"].with_user(user_b).search([])
        )
        self.assertIn(self.domain_b, seen_by_b)
        self.assertNotIn(self.domain_a, seen_by_b)
        self.assertNotIn(self.domain_c, seen_by_b)
        with self.assertRaises(AccessError):
            self.domain_a.with_user(user_b).read(["name"])

    def test_14_log_multi_company_isolation(self):
        """
        ses_webhook_log_comp_rule got the identical base.group_user
        scoping treatment as the domain rule -- prove ses.webhook.log
        isolation the same way, both directions, which was untested
        entirely before this fix.
        """
        # Create the log records as the module's own real service account
        # (matches how webhook_api.py actually writes them), then check
        # visibility as plain per-company personas.
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "ses_webhook.user_ses_webhook_service_internal"
        )
        log_a = self.env["ses.webhook.log"].with_user(svc_uid).create({
            "name": "log-isolation-a",
            "payload_type": "Notification",
            "raw_payload": "{}",
            "domain_id": self.domain_a.id,
            "status": "success",
        })
        log_b = self.env["ses.webhook.log"].with_user(svc_uid).create({
            "name": "log-isolation-b",
            "payload_type": "Notification",
            "raw_payload": "{}",
            "domain_id": self.domain_b.id,
            "status": "success",
        })

        user_a = self.env["res.users"].create({
            "name": "SES Webhook Log User A",
            "login": "ses_webhook_log_user_a",
            "company_id": self.company_a.id,
            "company_ids": [(6, 0, [self.company_a.id])],
            "group_ids": [(6, 0, [self.env.ref("base.group_user").id])],
        })
        user_b = self.env["res.users"].create({
            "name": "SES Webhook Log User B",
            "login": "ses_webhook_log_user_b",
            "company_id": self.company_b.id,
            "company_ids": [(6, 0, [self.company_b.id])],
            "group_ids": [(6, 0, [self.env.ref("base.group_user").id])],
        })

        seen_by_a = self.env["ses.webhook.log"].with_user(user_a).search([])
        self.assertIn(log_a, seen_by_a)
        self.assertNotIn(log_b, seen_by_a)

        seen_by_b = self.env["ses.webhook.log"].with_user(user_b).search([])
        self.assertIn(log_b, seen_by_b)
        self.assertNotIn(log_a, seen_by_b)

    def test_18_portal_and_public_denied_at_acl_layer(self):
        """
        Regression guard, not a bug-confirmation: no ir.model.access.csv
        row grants base.group_portal or the public user anything on
        either model today, so this is already true -- but that fact is
        currently implicit (no test asserts it). Pinning it down here so
        a future change (an LLM widening an ACL row without recognizing
        the consequence, for instance) is caught immediately rather than
        discovered later as a real leak. Per MASTER_12 Section 8's
        multi-persona mandate.
        """
        portal_user = self.env["res.users"].create({
            "name": "SES Webhook Portal Persona",
            "login": "ses_webhook_portal_persona",
            "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
        })
        public_user = self.env.ref("base.public_user")

        self._assert_denied_at_acl_layer(portal_user)
        self._assert_denied_at_acl_layer(public_user)

    def _assert_denied_at_acl_layer(self, persona):
        with self.assertRaises(AccessError):
            self.env["ses.webhook.domain"].with_user(persona).search([])
        with self.assertRaises(AccessError):
            self.env["ses.webhook.log"].with_user(persona).search([])
        with self.assertRaises(AccessError):
            self.env["ses.webhook.domain"].with_user(persona).create({
                "name": f"acl-probe-{persona.login}.com",
                "company_id": self.company_a.id,
            })
            self.env.flush_all()

    # ------------------------------------------------------------------
    # INCLUSION: the mechanism that's supposed to work, works, at scale.
    # ------------------------------------------------------------------

    def test_15_service_account_company_ids_synced_on_domain_create(self):
        # Tests [@ANCHOR: ses_webhook:COMM_domain_create]

        # Tests [@ANCHOR: ses_webhook:COMM_sync_service_account_companies]
        """
        ses_webhook_domain.py's create() override must grow the service
        account's company_ids the moment a domain is configured for a
        company it doesn't already cover -- otherwise with_company() in
        webhook_api.py raises AccessError the first time that tenant's
        webhook actually fires. Company D is untouched by every other
        test in this file, so this proves the growth is real, not
        already-covered by fixture setup order.
        """
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "ses_webhook.user_ses_webhook_service_internal"
        )
        svc_user = self.env["res.users"].browse(svc_uid)
        self.assertNotIn(self.company_d, svc_user.company_ids)

        self.env["ses.webhook.domain"].create({
            "name": "test-d.com",
            "secret_token": "mock_secret_d_sync_test",
            "company_id": self.company_d.id,
        })

        self.assertIn(
            self.company_d,
            svc_user.company_ids,
            "Creating a domain for a new company MUST grow the service "
            "account's company_ids automatically -- no hardcoded count.",
        )

    def test_15b_write_company_id_syncs_service_account(self):
        # Tests [@ANCHOR: ses_webhook:COMM_domain_write]
        """write()'s own sync path is distinct from create()'s -- moving an
        existing domain to a company the service account doesn't yet cover
        must grow company_ids the same way create() does."""
        new_company = self.env['res.company'].create({'name': 'Company E (write test)'})
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "ses_webhook.user_ses_webhook_service_internal"
        )
        svc_user = self.env["res.users"].browse(svc_uid)
        self.assertNotIn(new_company, svc_user.company_ids)

        domain = self.env["ses.webhook.domain"].create({
            "name": "test-e.com",
            "secret_token": "mock_secret_e_write_test",
            "company_id": self.company_a.id,
        })
        domain.write({"company_id": new_company.id})

        self.assertIn(
            new_company,
            svc_user.company_ids,
            "write()'s company_id change MUST grow the service account's "
            "company_ids the same way create() does.",
        )

    def test_16_security_backfill_action_syncs_pre_existing_domains(self):
        """
        The upgrade-safety path, not the fresh-install path: reset the
        service account's company_ids to just base.main_company (as if
        this were a database upgraded from before this fix), invoke
        ses_webhook_security_backfill_action directly (the same action
        data/security_backfill.xml's <function name="run"> triggers on
        every module load), and confirm it re-syncs every already-existing
        domain's company -- not just ones created after the fix.
        """
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "ses_webhook.user_ses_webhook_service_internal"
        )
        svc_user = self.env["res.users"].browse(svc_uid)
        svc_user.write({
            "company_ids": [(6, 0, [self.env.ref("base.main_company").id])]
        })
        self.assertNotIn(self.company_b, svc_user.company_ids)
        self.assertNotIn(self.company_c, svc_user.company_ids)

        self.env.ref("ses_webhook.ses_webhook_security_backfill_action").run()

        self.assertIn(self.company_a, svc_user.company_ids)
        self.assertIn(self.company_b, svc_user.company_ids)
        self.assertIn(self.company_c, svc_user.company_ids)

        rule = self.env.ref("ses_webhook.ses_webhook_domain_comp_rule")
        self.assertIn(self.env.ref("base.group_user"), rule.groups)

    def test_17_service_account_sees_across_untracked_company(self):
        """
        The key end-to-end proof of tenant-count independence. Company D
        and its domain are created fresh here, referenced nowhere else in
        any code or config -- the mechanism has to work for them purely
        because they exist, not because anything enumerates them.

        Drives the real controller exactly like test_04/test_05
        (message_process mocked, per this file's established pattern),
        and asserts on log.status == 'success', not just HTTP 200: the
        existing catch-all returns 200 on failure too, so only the real
        status field actually falsifies the with_company()/company_ids
        regression this fix targets.
        """
        domain_d = self.env["ses.webhook.domain"].create({
            "name": "test-d-e2e.com",
            "secret_token": "mock_secret_d_e2e",
            "company_id": self.company_d.id,
        })

        # The registration gate (SES_WEBHOOK_SENDER_REGISTRATION.md) now
        # requires a real matched sender to reach message_process() at
        # all -- register one here so this test still exercises what it's
        # actually about (the with_company()/company_ids sync), not the
        # gate itself (covered separately by test_19/test_20).
        self.env["res.users"].create({
            "name": "Matched Sender D",
            "login": "matched_sender_d",
            "email": "d@test-d-e2e.com",
            "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
        })

        raw_email = b"From: d@test-d-e2e.com\nTo: c@d.com\nSubject: Test D\n\nTest"
        ses_message = {"notificationType": "Received", "content": raw_email.decode("utf-8")}
        payload = {"Type": "Notification", "MessageId": "msg-notif-d", "Message": json.dumps(ses_message)}
        payload = self._sign(payload)

        mock_process = self.safe_patch("odoo.addons.mail.models.mail_thread.MailThread.message_process")
        mock_process.return_value = True
        response = self.url_open(
            f"/mail/webhook/sns?token={domain_d.secret_token}",
            data=json.dumps(payload).encode("utf-8"),
        )
        self.assertEqual(response.status_code, 200)

        log = self.env["ses.webhook.log"].search([("name", "=", "msg-notif-d")])
        self.assertEqual(len(log), 1)
        self.assertEqual(
            log.status,
            "success",
            f"Expected success, got '{log.status}': {log.error_message}. "
            "A failure here means the service account's company_ids "
            "wasn't actually synced for Company D -- the exact "
            "with_company()/AccessError regression this fix targets.",
        )
        self.assertEqual(log.domain_id, domain_d)
        self.assertEqual(log.company_id, self.company_d)

        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "ses_webhook.user_ses_webhook_service_internal"
        )
        seen_by_svc = self.env["ses.webhook.domain"].with_user(svc_uid).search([])
        self.assertIn(self.domain_a, seen_by_svc)
        self.assertIn(self.domain_b, seen_by_svc)
        self.assertIn(self.domain_c, seen_by_svc)
        self.assertIn(domain_d, seen_by_svc)

        user_a = self.env["res.users"].create({
            "name": "SES Webhook User A Post-D",
            "login": "ses_webhook_user_a_post_d",
            "company_id": self.company_a.id,
            "company_ids": [(6, 0, [self.company_a.id])],
            "group_ids": [(6, 0, [self.env.ref("base.group_user").id])],
        })
        seen_by_a = self.env["ses.webhook.domain"].with_user(user_a).search([])
        self.assertNotIn(
            domain_d,
            seen_by_a,
            "The service account seeing every tenant must NOT mean an "
            "ordinary base.group_user persona does too -- isolation for "
            "companies created AFTER install must hold exactly the same "
            "as for ones that existed at setUpClass time.",
        )

    def test_08_domain_unique_constraints(self):
        """Verify that SQL constraints block duplicate domain names and tokens."""
        # Due to how Odoo tests wrap transactions, checking SQL constraints requires mute_logger and catching IntegrityError
        with mute_logger('odoo.sql_db'), self.assertRaises(Exception):
            with self.env.cr.savepoint():
                self.env['ses.webhook.domain'].create({
                    'name': 'test-a.com', # Duplicate name
                    'company_id': self.company_b.id
                })
                self.env.flush_all()

        with mute_logger('odoo.sql_db'), self.assertRaises(Exception):
            with self.env.cr.savepoint():
                self.env['ses.webhook.domain'].create({
                    'name': 'test-c.com',
                    'secret_token': 'mock_secret_a', # Duplicate token
                    'company_id': self.company_b.id
                })
                self.env.flush_all()

    def test_11_views_rendering(self):
        # Tests [@ANCHOR: COMM_ses_webhook_views_render]
        """Proves all 4 ses_webhook backend views compile cleanly
        against their real models -- see ses_webhook_views.xml's
        audit-ignore-view comments for why these skip a browser tour
        (plain admin config list/forms, no client-side logic to
        exercise beyond standard field rendering)."""
        v1 = self.env["ses.webhook.domain"].get_view(view_type="list")
        self.assertIn('name="webhook_url"', v1["arch"])

        v2 = self.env["ses.webhook.domain"].get_view(view_type="form")
        self.assertIn('name="secret_token"', v2["arch"])

        v3 = self.env["ses.webhook.log"].get_view(view_type="list")
        self.assertIn('name="payload_type"', v3["arch"])

        v4 = self.env["ses.webhook.log"].get_view(view_type="form")
        self.assertIn('name="raw_payload"', v4["arch"])

        v5 = self.env["ses.webhook.pending_submission"].get_view(view_type="list")
        self.assertIn('name="sender_email"', v5["arch"])

    def test_12_processing_failure_still_returns_200_and_logs(self):
        # Tests [@ANCHOR: COMM_ses_webhook_process_catch_all]
        """
        receive_sns_webhook()'s broad except Exception must still return
        200 to AWS (a non-2xx response makes SNS retry indefinitely) even
        when processing genuinely fails, and must record the real failure
        in ses.webhook.log rather than swallowing it silently. Force a
        real failure (message_process raising) and prove both halves.
        """
        raw_email = b"From: a@test-a.com\nTo: c@d.com\nSubject: Test Failure\n\nTest"
        ses_message = {"notificationType": "Received", "content": raw_email.decode('utf-8')}
        payload = {"Type": "Notification", "MessageId": "msg-forced-failure", "Message": json.dumps(ses_message)}
        payload = self._sign(payload)

        mock_process = self.safe_patch('odoo.addons.mail.models.mail_thread.MailThread.message_process')
        mock_process.side_effect = RuntimeError("simulated processing failure")

        response = self.url_open(
            f'/mail/webhook/sns?token={self.domain_a.secret_token}',
            data=json.dumps(payload).encode('utf-8'),
        )
        self.assertEqual(
            response.status_code,
            200,
            "A processing failure must still return 200 to AWS, or SNS "
            "will retry indefinitely.",
        )

        log = self.env['ses.webhook.log'].search([('name', '=', 'msg-forced-failure')])
        self.assertEqual(len(log), 1)
        self.assertEqual(log.status, 'failed')
        self.assertIn('simulated processing failure', log.error_message)

    def test_19_unmatched_sender_is_gated_not_processed(self):
        # Tests [@ANCHOR: ses_webhook:COMM_create_and_notify]

        # Tests [@ANCHOR: ses_webhook:COMM_pending_submission_compute_name]
        """
        SES_WEBHOOK_SENDER_REGISTRATION.md: an unmatched sender must never
        reach message_process() -- a real, unregistered address, verified
        end to end: message_process() is never called, a
        ses.webhook.pending_submission is created with the right sender/
        domain/token, a real mail.mail nudge is created and its send()
        attempted (real SMTP is unreachable in this sandbox, so it ends in
        state 'exception', not 'sent' -- that's the honest signal of "the
        gate itself worked," not a mock standing in for delivery), and the
        log records 'ignored', not 'success'.
        """
        raw_email = b"From: nobody-registered@test-a.com\nTo: c@d.com\nSubject: Unmatched\n\nTest"
        ses_message = {"notificationType": "Received", "content": raw_email.decode('utf-8')}
        payload = {"Type": "Notification", "MessageId": "msg-unmatched", "Message": json.dumps(ses_message)}
        payload = self._sign(payload)

        mock_process = self.safe_patch('odoo.addons.mail.models.mail_thread.MailThread.message_process')

        response = self.url_open(
            f'/mail/webhook/sns?token={self.domain_a.secret_token}',
            data=json.dumps(payload).encode('utf-8'),
        )
        self.assertEqual(response.status_code, 200)
        mock_process.assert_not_called()

        log = self.env['ses.webhook.log'].search([('name', '=', 'msg-unmatched')])
        self.assertEqual(len(log), 1)
        self.assertEqual(log.status, 'ignored')
        self.assertIn('nobody-registered@test-a.com', log.error_message)

        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "ses_webhook.user_ses_webhook_service_internal"
        )
        submission = self.env['ses.webhook.pending_submission'].with_user(svc_uid).search(
            [('sender_email', '=', 'nobody-registered@test-a.com')]
        )
        self.assertEqual(len(submission), 1)
        self.assertEqual(submission.domain_id, self.domain_a)
        self.assertEqual(submission.company_id, self.company_a)
        self.assertFalse(submission.consumed)
        self.assertTrue(submission.token)
        self.assertIn('nobody-registered@test-a.com', submission.name)

        mail = self.env['mail.mail'].search([('email_to', '=', 'nobody-registered@test-a.com')])
        self.assertEqual(len(mail), 1)
        self.assertIn('/web/signup', mail.body_html)
        self.assertIn(mail.state, ('exception', 'outgoing', 'sent'))

    def test_20_matched_sender_is_not_gated(self):
        """
        The registration gate must not interfere with a real, registered
        sender -- message_process() still gets called, and no pending
        submission is created for them. (test_04/05/12 already prove
        message_process() runs end to end for a matched sender; this test
        isolates the gate's own "no pending submission" side specifically.)
        """
        raw_email = b"From: a@test-a.com\nTo: c@d.com\nSubject: Matched\n\nTest"
        ses_message = {"notificationType": "Received", "content": raw_email.decode('utf-8')}
        payload = {"Type": "Notification", "MessageId": "msg-matched", "Message": json.dumps(ses_message)}
        payload = self._sign(payload)

        mock_process = self.safe_patch('odoo.addons.mail.models.mail_thread.MailThread.message_process')
        mock_process.return_value = True

        response = self.url_open(
            f'/mail/webhook/sns?token={self.domain_a.secret_token}',
            data=json.dumps(payload).encode('utf-8'),
        )
        self.assertEqual(response.status_code, 200)
        mock_process.assert_called_once()

        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "ses_webhook.user_ses_webhook_service_internal"
        )
        submission = self.env['ses.webhook.pending_submission'].with_user(svc_uid).search(
            [('sender_email', '=', 'a@test-a.com')]
        )
        self.assertFalse(submission)

    def test_20b_log_cron_truncates_old_rows_only(self):
        # Tests [@ANCHOR: ses_webhook:COMM_cron_truncate_logs]
        """_cron_truncate_logs() must delete rows past the 30-day window
        and leave recent ones alone -- both directions, matching the same
        proof _cron_truncate_pending_submissions() gets below."""
        old = self.env['ses.webhook.log'].create({
            'name': 'old-log-msg',
            'payload_type': 'Notification',
            'status': 'success',
            'domain_id': self.domain_a.id,
        })
        recent = self.env['ses.webhook.log'].create({
            'name': 'recent-log-msg',
            'payload_type': 'Notification',
            'status': 'success',
            'domain_id': self.domain_a.id,
        })
        self.env.cr.execute(
            "UPDATE ses_webhook_log SET create_date = create_date - interval '31 days' WHERE id = %s",
            (old.id,),
        )

        self.env['ses.webhook.log'].with_user(
            self.env.ref('base.user_root').id
        )._cron_truncate_logs()

        self.assertFalse(old.exists())
        self.assertTrue(recent.exists())

    def test_21_pending_submission_cron_truncates_old_rows_only(self):
        # Tests [@ANCHOR: ses_webhook:COMM_cron_truncate_pending_submissions]
        """_cron_truncate_pending_submissions() must delete rows past the
        7-day window and leave recent ones alone -- both directions, not
        just "it doesn't crash."""
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "ses_webhook.user_ses_webhook_service_internal"
        )
        old = self.env['ses.webhook.pending_submission'].with_user(svc_uid).create({
            'sender_email': 'old@test-a.com',
            'raw_content': 'old',
            'domain_id': self.domain_a.id,
        })
        recent = self.env['ses.webhook.pending_submission'].with_user(svc_uid).create({
            'sender_email': 'recent@test-a.com',
            'raw_content': 'recent',
            'domain_id': self.domain_a.id,
        })
        self.env.cr.execute(
            "UPDATE ses_webhook_pending_submission SET create_date = create_date - interval '8 days' WHERE id = %s",
            (old.id,),
        )

        # Matches ir_cron.xml's own user_id (base.user_root) -- the real
        # cron runs as root, not the service account, so this test does too.
        self.env['ses.webhook.pending_submission'].with_user(
            self.env.ref('base.user_root').id
        )._cron_truncate_pending_submissions()

        self.assertFalse(old.exists())
        self.assertTrue(recent.exists())

    def test_22_pending_submission_multi_company_isolation(self):
        """Same multi-company ir.rule treatment as domains/logs -- prove
        it holds for the new model too, not just assumed from the copied
        XML shape."""
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "ses_webhook.user_ses_webhook_service_internal"
        )
        sub_a = self.env['ses.webhook.pending_submission'].with_user(svc_uid).create({
            'sender_email': 'iso-a@test-a.com',
            'raw_content': 'x',
            'domain_id': self.domain_a.id,
        })
        sub_b = self.env['ses.webhook.pending_submission'].with_user(svc_uid).create({
            'sender_email': 'iso-b@test-b.com',
            'raw_content': 'x',
            'domain_id': self.domain_b.id,
        })

        user_a = self.env["res.users"].create({
            "name": "SES Webhook Pending User A",
            "login": "ses_webhook_pending_user_a",
            "company_id": self.company_a.id,
            "company_ids": [(6, 0, [self.company_a.id])],
            "group_ids": [(6, 0, [self.env.ref("base.group_user").id])],
        })

        seen_by_a = self.env['ses.webhook.pending_submission'].with_user(user_a).search([])
        self.assertIn(sub_a, seen_by_a)
        self.assertNotIn(sub_b, seen_by_a)
        with self.assertRaises(AccessError):
            sub_b.with_user(user_a).read(["sender_email"])
