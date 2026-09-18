# -*- coding: utf-8 -*-
from email.message import EmailMessage

from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

@tagged('post_install', '-at_install')
class TestMailThread(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.env['ir.config_parameter'].with_user(self.env.ref('base.user_admin').id).set_param('mail.bounce.alias', 'auto-mail-failure')

    def test_vacation_reply_dropped(self):
        # Tests [@ANCHOR: hams_base:COMM_message_route]
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

        mock_logger = self.safe_patch('odoo.addons.hams_base.models.mail_thread._logger')
        with self.assertRaises(ValueError):
            self.env['mail.thread'].message_route(real_message, msg_dict)
        for call in mock_logger.info.call_args_list:
            self.assertNotIn(
                'Dropping', call.args[0],
                "A genuine (non-noise) postmaster@ message must not hit any "
                "of this override's drop branches.",
            )

    def test_dmarc_reports_alias_is_wired_and_routes_to_the_real_model(self):
        """Bug-hunt fix, 2026-09-18, per Bruce's own answer in
        night_shift_questions/answered/
        hams-base-dmarc-pipeline-unwired-and-poisoned-alias-310b7a4f.md: no mail.alias used to
        route anything to hams_base.dmarc.report, so a real DMARC aggregate report had no path
        to ever reach the handler. Unlike the postmaster test above (which relies on a
        DIFFERENT module's alias and isn't installed in this module's own isolated test
        environment), hams_base now ships its own dmarc-reports@ alias directly
        (data/mail_alias_data.xml) -- this test proves it for real, not just that routing
        "falls through"."""
        # Tests [@ANCHOR: hams_base:COMM_message_route]
        alias = self.env['mail.alias'].search([('alias_name', '=', 'dmarc-reports')])
        self.assertTrue(alias, "the dmarc-reports@ mail.alias must exist")
        self.assertEqual(
            alias.alias_model_id.model,
            'hams_base.dmarc.report',
            "dmarc-reports@ must route to hams_base.dmarc.report",
        )

        # message_route()'s own alias lookup matches on alias_full_name
        # (alias_name@alias_domain_id.name) unless alias_incoming_local is
        # set, so the alias record needs a real alias_domain_id to match a
        # real "@hams.com" recipient -- this module's own data file leaves
        # alias_domain_id to its ORM default (the company's own
        # alias_domain_id at install time), which is unset in this
        # isolated test database. Real production relies on a real
        # mail.alias.domain already configured via normal Odoo
        # administration (Settings > Technical > Email > Alias Domains),
        # the same way pager_duty/tests/test_mail_ingest_incident.py's own
        # setUpClass configures it for its own postmaster@ alias test.
        # Set it explicitly here rather than relying on install-time
        # ordering.
        alias_domain = self.env['mail.alias.domain'].search([('name', '=', 'hams.com')], limit=1)
        if not alias_domain:
            alias_domain = self.env['mail.alias.domain'].create({'name': 'hams.com'})
        alias.alias_domain_id = alias_domain.id

        msg_dict = {
            'to': 'dmarc-reports@hams.com',
            'subject': 'Report Domain: example.com Submitter: mail.example.net',
            'body': '',
            'email_from': 'noreply-dmarc-support@google.com',
            'message_id': '<test-dmarc-reports@google.com>',
            'references': '',
            'in_reply_to': '',
            'recipients': 'dmarc-reports@hams.com',
        }
        real_message = EmailMessage()
        real_message['To'] = msg_dict['to']
        real_message['From'] = msg_dict['email_from']
        real_message['Subject'] = msg_dict['subject']
        real_message['Message-Id'] = msg_dict['message_id']
        real_message.set_content(msg_dict['body'])

        mock_logger = self.safe_patch('odoo.addons.hams_base.models.mail_thread._logger')
        # Real alias resolution -- unlike the postmaster test, this alias IS
        # installed here, so this must resolve to a real route, not raise.
        routes = self.env['mail.thread'].message_route(real_message, msg_dict)
        self.assertTrue(routes, "a message to the real dmarc-reports@ alias must resolve to a route")
        self.assertEqual(routes[0][0], 'hams_base.dmarc.report')
        for call in mock_logger.info.call_args_list:
            self.assertNotIn(
                'Dropping', call.args[0],
                "dmarc-reports@ is not special-cased in message_route and must never be dropped.",
            )
