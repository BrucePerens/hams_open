# SPDX-License-Identifier: AGPL-3.0-or-later
import json
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from unittest.mock import patch

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
        with patch('urllib.request.urlopen') as mock_urlopen:
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
        
        with patch('odoo.addons.mail.models.mail_thread.MailThread.message_process') as mock_process:
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
        
        with patch('odoo.addons.mail.models.mail_thread.MailThread.message_process') as mock_process:
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

    def test_08_domain_unique_constraints(self):
        """Verify that SQL constraints block duplicate domain names and tokens."""
        # Due to how Odoo tests wrap transactions, checking SQL constraints requires mute_logger and catching IntegrityError
        from odoo.tools import mute_logger
        
        with mute_logger('odoo.sql_db'), self.assertRaises(Exception):
            with self.env.cr.savepoint():
                self.env['ses.webhook.domain'].create({
                    'name': 'test-a.com', # Duplicate name
                    'company_id': self.company_b.id
                })
                
        with mute_logger('odoo.sql_db'), self.assertRaises(Exception):
            with self.env.cr.savepoint():
                self.env['ses.webhook.domain'].create({
                    'name': 'test-c.com',
                    'secret_token': 'mock_secret_a', # Duplicate token
                    'company_id': self.company_b.id
                })
