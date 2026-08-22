# SPDX-License-Identifier: AGPL-3.0-or-later
import json
from odoo.addons.zero_sudo.tests.common import HamsHttpCase

from odoo.exceptions import AccessError
from odoo.tests.common import tagged
from odoo.tools import mute_logger

@tagged('post_install', '-at_install')
class TestSesWebhook(HamsHttpCase):

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        # Create two companies for multi-tenant testing
        cls.company_a = cls.env.company
        cls.company_b = cls.env['res.company'].create({'name': 'Company B'})
        
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

    def test_01_webhook_unauthorized(self):
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
            "SubscribeURL": "http://mock-aws.com/confirm"
        }
        mock_urlopen = self.safe_patch('urllib.request.urlopen')
        mock_urlopen.return_value = True
        response = self.url_open(f'/mail/webhook/sns?token={self.domain_a.secret_token}', data=json.dumps(payload).encode('utf-8'))
        self.assertEqual(response.status_code, 200)
        mock_urlopen.assert_called_once_with("http://mock-aws.com/confirm")

        log = self.env['ses.webhook.log'].search([('name', '=', 'msg-sub-1')])
        self.assertEqual(len(log), 1)
        self.assertEqual(log.status, 'success')
        self.assertEqual(log.domain_id, self.domain_a)

    def test_04_webhook_notification_processed_company_a(self):
        """Verify Notification extracts content and passes to mail.thread in Company A context."""
        raw_email = b"From: a@test-a.com\nTo: c@d.com\nSubject: Test A\n\nTest"
        ses_message = {"notificationType": "Received", "content": raw_email.decode('utf-8')}
        payload = {"Type": "Notification", "MessageId": "msg-notif-a", "Message": json.dumps(ses_message)}
        
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
        
        response = self.url_open(f'/mail/webhook/sns?token={self.domain_a.secret_token}', data=json.dumps(payload).encode('utf-8'))
        self.assertEqual(response.status_code, 200) # Returns 200 to AWS to stop retries
        
        log = self.env['ses.webhook.log'].search([('name', '=', 'msg-no-content')])
        self.assertEqual(len(log), 1)
        self.assertEqual(log.status, 'ignored')
        self.assertIn('No content field found', log.error_message)

    def test_07_webhook_unsubscribe_confirmation(self):
        """Verify UnsubscribeConfirmation is ignored properly."""
        payload = {"Type": "UnsubscribeConfirmation", "MessageId": "msg-unsub"}
        
        response = self.url_open(f'/mail/webhook/sns?token={self.domain_a.secret_token}', data=json.dumps(payload).encode('utf-8'))
        self.assertEqual(response.status_code, 200)
        
        log = self.env['ses.webhook.log'].search([('name', '=', 'msg-unsub')])
        self.assertEqual(len(log), 1)
        self.assertEqual(log.status, 'ignored')

    def test_09_webhook_url_computes_for_plain_internal_user(self):
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
        """
        plain_user = self.env["res.users"].create({
            "name": "Plain Internal Viewer",
            "login": "ses_webhook_plain_viewer",
            "group_ids": [(6, 0, [self.env.ref("base.group_user").id])],
        })
        self.assertFalse(plain_user.has_group("base.group_system"))

        domain_as_plain_user = self.domain_a.with_user(plain_user)
        self.assertTrue(
            domain_as_plain_user.webhook_url,
            "A plain internal user (base.group_user) MUST be able to "
            "compute/read webhook_url on a record their own ACL grants "
            "them read access to.",
        )

    def test_10_domain_multi_company_isolation(self):
        """
        ses_webhook_domain_comp_rule is a GLOBAL rule (no groups field:
        '|', company_id=False, company_id in company_ids), so it ANDs
        with every user's access, including plain base.group_user (which
        has read-only ACL access to ses.webhook.domain). Nothing had ever
        proven it actually isolates two companies from each other.
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
        with self.assertRaises(AccessError):
            self.domain_b.with_user(user_a).read(["name"])

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
