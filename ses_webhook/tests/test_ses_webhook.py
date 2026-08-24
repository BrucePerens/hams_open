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

    def test_21_pending_submission_cron_truncates_old_rows_only(self):
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
