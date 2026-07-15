# -*- coding: utf-8 -*-
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
