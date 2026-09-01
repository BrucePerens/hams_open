# -*- coding: utf-8 -*-
from email.message import EmailMessage
from unittest import mock

from odoo.tests import common, tagged

@tagged('post_install', '-at_install')
class TestMailThread(common.TransactionCase):
    def setUp(self):
        super().setUp()
        self.env['ir.config_parameter'].with_user(self.env.ref('base.user_admin').id).set_param('mail.bounce.alias', 'auto-mail-failure')

    def test_vacation_reply_dropped(self):
        msg_dict = {
            'to': 'not-read@hams.com',
            'subject': 'Out of Office: Thank you',
            'body': 'I am away.',
            'email_from': 'user@example.com'
        }
        result = self.env['mail.thread'].message_route('Out of Office: Thank you', msg_dict)
        self.assertEqual(result, [])

    def test_unsubscribe_intent_dropped(self):
        msg_dict = {
            'to': 'auto-mail-failure@hams.com',
            'subject': 'Unsubscribe me please',
            'body': 'Stop emailing me.',
            'email_from': 'user@example.com'
        }
        result = self.env['mail.thread'].message_route('Unsubscribe me please', msg_dict)
        self.assertEqual(result, [])

    def test_postmaster_vacation_reply_dropped(self):
        # postmaster@ gets the same DSN-bounce/vacation-reply noise
        # filtering as the dedicated bounce alias and not-read@ (see
        # message_route()'s own is_postmaster_route handling) -- it's the
        # RFC 5321-mandated admin contact address for the domain, so it
        # genuinely receives this kind of automated noise too.
        msg_dict = {
            'to': 'postmaster@hams.com',
            'subject': 'Out of Office: Thank you',
            'body': 'I am away.',
            'email_from': 'user@example.com'
        }
        result = self.env['mail.thread'].message_route('Out of Office: Thank you', msg_dict)
        self.assertEqual(result, [])

    def test_postmaster_unsubscribe_intent_dropped(self):
        msg_dict = {
            'to': 'postmaster@hams.com',
            'subject': 'Unsubscribe me please',
            'body': 'Stop emailing me.',
            'email_from': 'user@example.com'
        }
        result = self.env['mail.thread'].message_route('Unsubscribe me please', msg_dict)
        self.assertEqual(result, [])

    def test_postmaster_genuine_inquiry_falls_through_to_normal_routing(self):
        # Unlike not-read@, a genuine (non-bounce, non-unsubscribe,
        # non-vacation) message to postmaster@ must NOT be silently
        # dropped -- it needs to fall through to super().message_route()
        # so a real mail.alias (pager_duty's postmaster@ -> pager.incident,
        # not present in this module's own isolated test install) can turn
        # it into a ticket. With no such alias registered here, Odoo's own
        # base message_route() correctly raises ValueError ("no possible
        # route found") for a genuinely unmatched recipient -- confirmed
        # directly, not assumed -- which is actually the clean proof this
        # test needs: if this override's own unsubscribe/vacation/
        # not-read-catch-all branches had incorrectly swallowed this
        # genuine inquiry (returning [] instead of falling through), no
        # ValueError would ever occur.
        msg_dict = {
            'to': 'postmaster@hams.com',
            'subject': 'Question about your mail server configuration',
            'body': 'Hello, I run a mail server and had a question.',
            'email_from': 'other-admin@example.net',
            # Odoo's base message_route() reads all of these keys directly
            # (bracket access, not .get()) once it's actually reached --
            # the drop-path tests above never get this far, so they get
            # away with a minimal dict; this one needs the real shape.
            'message_id': '<test-postmaster-genuine@example.net>',
            'references': '',
            'in_reply_to': '',
            'recipients': 'postmaster@hams.com',
        }
        # Odoo's own base message_route() (past this override's super() call)
        # requires a real email.message.EmailMessage, not the bare string the
        # drop-path tests above get away with -- those never reach super() at
        # all. A minimal real message with matching headers, not a mock, so
        # this exercises the actual base routing code, not a stand-in for it.
        real_message = EmailMessage()
        real_message['To'] = msg_dict['to']
        real_message['From'] = msg_dict['email_from']
        real_message['Subject'] = msg_dict['subject']
        real_message['Message-Id'] = '<test-postmaster-genuine@example.net>'
        real_message.set_content(msg_dict['body'])

        with mock.patch(
            'odoo.addons.hams_base.models.mail_thread._logger'
        ) as mock_logger:
            with self.assertRaises(ValueError):
                self.env['mail.thread'].message_route(real_message, msg_dict)
        for call in mock_logger.info.call_args_list:
            self.assertNotIn(
                'Dropping', call.args[0],
                "A genuine (non-noise) postmaster@ message must not hit any "
                "of this override's drop branches.",
            )
