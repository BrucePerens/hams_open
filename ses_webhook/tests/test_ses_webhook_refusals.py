# SPDX-License-Identifier: AGPL-3.0-or-later
"""Refusal branches of the SNS webhook that no other test reached: a Type AWS does not sign, a missing signed
field, any error while verifying (fail closed), a JSON body that is not an object, and an attacker-chosen Type
that must never reach the log's Selection field."""
import json

from cryptography.hazmat.primitives.asymmetric import rsa

from odoo.addons.ses_webhook.controllers import webhook_api
from odoo.addons.ses_webhook.tests.test_ses_webhook import _TEST_SIGNING_CERT_URL, _make_self_signed_cert, _sign_sns_payload
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests.common import tagged


@tagged('post_install', '-at_install')
class TestSnsRefusals(HamsHttpCase):
    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.private_key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
        cls.cert_pem = _make_self_signed_cert(cls.private_key)
        cls.domain = cls.env['ses.webhook.domain'].create({
            'name': 'refusals-a.com', 'secret_token': 'refusals_secret', 'company_id': cls.env.company.id,
        })

    def setUp(self):
        super().setUp()
        self.fetch = self.safe_patch('odoo.addons.ses_webhook.controllers.webhook_api._fetch_sns_signing_cert')
        self.fetch.return_value = self.cert_pem

    def _signed_notification(self, **extra):
        payload = {'Type': 'Notification', 'MessageId': 'msg-refusal', 'Message': 'hello'}
        payload.update(extra)
        return _sign_sns_payload(payload, self.private_key)

    def test_a_signed_notification_verifies(self):
        # Tests [@ANCHOR: ses_webhook:COMM_verify_sns_signature]
        self.assertTrue(webhook_api._verify_sns_signature(self._signed_notification()))

    def test_a_type_aws_does_not_sign_or_a_missing_signed_field_has_no_string_to_sign(self):
        # Tests [@ANCHOR: ses_webhook:build_string_to_sign]
        self.assertIsNone(webhook_api._build_string_to_sign({'Type': 'Bogus', 'Message': 'x'}))
        payload = self._signed_notification()
        del payload['MessageId']
        self.assertIsNone(webhook_api._build_string_to_sign(payload))

    def test_an_unsignable_type_is_refused_before_the_certificate_is_fetched(self):
        # Tests [@ANCHOR: ses_webhook:COMM_verify_sns_signature]
        payload = self._signed_notification()
        payload['Type'] = 'Bogus'
        self.assertFalse(webhook_api._verify_sns_signature(payload))
        self.fetch.assert_not_called()

    def test_a_certificate_fetch_failure_fails_closed(self):
        # Tests [@ANCHOR: ses_webhook:COMM_verify_sns_signature]
        self.fetch.side_effect = OSError('network down')
        with self.assertLogs('odoo.addons.ses_webhook.controllers.webhook_api', level='WARNING') as logs:
            self.assertFalse(webhook_api._verify_sns_signature(self._signed_notification()))
        self.assertIn('OSError', logs.output[0])

    def test_a_signature_that_is_not_base64_fails_closed(self):
        # Tests [@ANCHOR: ses_webhook:COMM_verify_sns_signature]
        payload = self._signed_notification()
        payload['Signature'] = '!!not base64!!'
        with self.assertLogs('odoo.addons.ses_webhook.controllers.webhook_api', level='WARNING'):
            self.assertFalse(webhook_api._verify_sns_signature(payload))

    def test_a_json_body_that_is_not_an_object_is_a_400_and_logs_nothing(self):
        # Tests [@ANCHOR: ses_webhook:COMM_receive_sns_webhook]
        before = self.env['ses.webhook.log'].search_count([])
        for body in (b'[]', b'"text"', b'7'):
            response = self.url_open('/mail/webhook/sns?token=refusals_secret', data=body)
            self.assertEqual(response.status_code, 400, body)
        self.assertEqual(self.env['ses.webhook.log'].search_count([]), before)

    def test_an_attacker_chosen_type_is_logged_as_unknown_and_refused(self):
        # Tests [@ANCHOR: ses_webhook:COMM_receive_sns_webhook]
        payload = {'Type': 'Bogus', 'MessageId': 'msg-bogus-type', 'Message': 'x', 'SigningCertURL': _TEST_SIGNING_CERT_URL}
        response = self.url_open('/mail/webhook/sns?token=refusals_secret', data=json.dumps(payload).encode('utf-8'))
        self.assertEqual(response.status_code, 403)
        log = self.env['ses.webhook.log'].search([('name', '=', 'msg-bogus-type')])
        self.assertEqual(log.payload_type, 'Unknown')
        self.assertEqual(log.status, 'rejected_signature')

    def test_a_json_escape_for_nul_or_a_lone_surrogate_is_a_400_and_logs_nothing(self):
        # Tests [@ANCHOR: ses_webhook:storable_payload]
        # The text of the body is valid JSON, but the string it decodes to cannot be stored: PostgreSQL has no NUL
        # and a lone surrogate cannot be encoded. The rejected_signature log row used to be created from it: a 500.
        before = self.env['ses.webhook.log'].search_count([])
        for message_id in ('a\\u0000b', 'a\\ud800b'):
            body = ('{"Type": "Notification", "MessageId": "%s", "Message": "x"}' % message_id).encode('ascii')
            response = self.url_open('/mail/webhook/sns?token=refusals_secret', data=body)
            self.assertEqual(response.status_code, 400, message_id)
        self.assertEqual(self.env['ses.webhook.log'].search_count([]), before)
