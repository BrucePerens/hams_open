# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
"""Tests for the NCMEC mandatory-reporting workflow (docs/proposals/
CHILD_SAFETY_COMMUNICATIONS_CONSENT.md, section G / Phase 8, models/helpdesk_ticket.py).

Never exercises a real network call: every real request in this file is patched at the
odoo.addons.hams_helpdesk.models.helpdesk_ticket._urlopen_ssrf_safe boundary, the same
zero_sudo.daemon.ssrf_safe_fetch seam binary_downloader's own tests already mock (see
test_binary_manifest.py) -- and hams_helpdesk.ncmec_api_base_url is never set to a real host
anywhere in this file, only to an obviously-fake ``https://ncmec-api-test.invalid/ispws`` (the
``.invalid`` TLD is reserved by RFC 2606 and is guaranteed to never resolve), so even a mocking
mistake could not reach a real NCMEC endpoint. Two independent, structural safety layers,
matching this task's own instruction to make a real submission during testing hard by
construction, not just disciplined.
"""
from unittest.mock import MagicMock
import io
import urllib.error

from odoo import _
from odoo.exceptions import AccessError, UserError
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.daemon.ssrf_safe_fetch import SSRFValidationError
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.hams_helpdesk.models.helpdesk_ticket import (
    _CSAM_TICKET_TYPE,
    _QSO_RECORDING_MODEL,
)

_FAKE_NCMEC_BASE_URL = "https://ncmec-api-test.invalid/ispws"


@tagged("post_install", "-at_install", "standard")
class TestNcmecReport(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.safe_patch_object(
            type(self.env["bus.bus"]), "_sendone", lambda *a, **kw: None, create=True
        )

        system_partner = self.env["res.partner"].create(
            {"name": "NCMEC Test Sysadmin", "email": "ncmec_sysadmin@example.com"}
        )
        self.system_user = self.env["res.users"].create(
            {
                "name": "NCMEC Test Sysadmin",
                "login": "ncmec_sysadmin_test",
                "partner_id": system_partner.id,
                "group_ids": [
                    (
                        6,
                        0,
                        [
                            self.env.ref("hams_helpdesk.group_helpdesk_manager").id,
                            self.env.ref("base.group_system").id,
                        ],
                    )
                ],
            }
        )
        self.manager_partner = self.env["res.partner"].create(
            {"name": "NCMEC Test Plain Manager", "email": "ncmec_manager@example.com"}
        )
        self.manager_user = self.env["res.users"].create(
            {
                "name": "NCMEC Test Plain Manager",
                "login": "ncmec_plain_manager_test",
                "partner_id": self.manager_partner.id,
                "group_ids": [
                    (6, 0, [self.env.ref("hams_helpdesk.group_helpdesk_manager").id])
                ],
            }
        )
        self.reported_partner = self.env["res.partner"].create(
            {
                "name": "NCMEC Test Reported Member",
                "email": "ncmec_reported@example.com",
                "callsign": self.get_callsign("NCMEC_REPORTED"),
            }
        )
        self.reported_user = self.env["res.users"].create(
            {
                "name": "NCMEC Test Reported Member",
                "login": "ncmec_reported_member_test",
                "partner_id": self.reported_partner.id,
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )

    def _create_csam_ticket(self, **extra_vals):
        vals = {"name": "Flagged QSO", "ticket_type": _CSAM_TICKET_TYPE}
        vals.update(extra_vals)
        return self.env["hams_helpdesk.ticket"].create(vals)

    # ------------------------------------------------------------------
    # Creation: forced priority, packet assembly, broad manager notification
    # ------------------------------------------------------------------

    def test_csam_ticket_creation_assembles_packet_and_forces_priority(self):
        # Tests [@ANCHOR: hams_helpdesk:COMM_ncmec_packet_and_legal_hold]
        # Tests [@ANCHOR: hams_helpdesk:COMM_ncmec_force_priority]
        # Tests [@ANCHOR: hams_helpdesk:COMM_ncmec_assemble_report_packet]
        uuid = "11111111-1111-1111-1111-111111111111"
        ticket = self._create_csam_ticket(
            partner_id=self.reported_partner.id, ncmec_recording_uuid=uuid
        )
        self.assertEqual(
            ticket.priority, "3", "A CSAM/enticement/trafficking ticket must be forced Critical."
        )
        self.assertEqual(ticket.ncmec_contact_email, "admin@hams.com")
        self.assertEqual(ticket.ncmec_contact_phone, "+1 510-473-7367")
        self.assertTrue(
            ticket.ncmec_recording_playback_url.endswith(
                f"/simulated-band/recordings/{uuid}"
            )
        )
        self.assertEqual(ticket.ncmec_reported_user_id, self.reported_user)
        self.assertIn(uuid, ticket.ncmec_report_packet)
        self.assertIn("admin@hams.com", ticket.ncmec_report_packet)
        self.assertIn("+1 510-473-7367", ticket.ncmec_report_packet)
        self.assertIn(self.reported_partner.callsign, ticket.ncmec_report_packet)

    def test_regular_ticket_is_not_forced_to_critical_priority(self):
        ticket = self.env["hams_helpdesk.ticket"].create({"name": "Ordinary ticket"})
        self.assertEqual(ticket.priority, "0")
        self.assertFalse(ticket.ncmec_report_packet)

    def test_csam_ticket_creation_notifies_every_helpdesk_manager(self):
        # Tests [@ANCHOR: hams_helpdesk:COMM_ncmec_notify_all_managers]
        ticket = self._create_csam_ticket()
        self.assertIn(self.system_user.partner_id, ticket.message_partner_ids)
        self.assertIn(self.manager_user.partner_id, ticket.message_partner_ids)

    def test_regular_ticket_does_not_notify_every_helpdesk_manager(self):
        ticket = self.env["hams_helpdesk.ticket"].create({"name": "Ordinary ticket"})
        self.assertNotIn(self.manager_user.partner_id, ticket.message_partner_ids)

    # ------------------------------------------------------------------
    # _ncmec_report_ticket_for_recording: the dedup-aware creation entrypoint
    # ------------------------------------------------------------------

    def test_report_ticket_for_recording_creates_a_new_ticket(self):
        # Tests [@ANCHOR: hams_helpdesk:COMM_ncmec_report_ticket_for_recording]
        uuid = "22222222-2222-2222-2222-222222222222"
        ticket = self.env["hams_helpdesk.ticket"]._ncmec_report_ticket_for_recording(
            uuid, {"name": "Auto-flagged by bot"}
        )
        self.assertEqual(ticket.ticket_type, _CSAM_TICKET_TYPE)
        self.assertEqual(ticket.priority, "3")
        self.assertEqual(ticket.ncmec_recording_uuid, uuid)

    def test_report_ticket_for_recording_attaches_to_an_existing_ticket(self):
        uuid = "33333333-3333-3333-3333-333333333333"
        Ticket = self.env["hams_helpdesk.ticket"]
        first = Ticket._ncmec_report_ticket_for_recording(uuid, {"name": "First flag"})
        second = Ticket._ncmec_report_ticket_for_recording(uuid, {"name": "Second flag"})
        self.assertEqual(first, second)
        self.assertEqual(
            Ticket.search_count([("ncmec_recording_uuid", "=", uuid)]),
            1,
            "A second flag on the same recording must not open a duplicate ticket.",
        )

    def test_report_ticket_for_recording_reopens_after_a_closed_prior_ticket(self):
        uuid = "44444444-4444-4444-4444-444444444444"
        Ticket = self.env["hams_helpdesk.ticket"]
        first = Ticket._ncmec_report_ticket_for_recording(uuid, {"name": "First flag"})
        first.with_user(self.system_user).write({"stage": "closed"})
        second = Ticket._ncmec_report_ticket_for_recording(uuid, {"name": "Second flag"})
        self.assertNotEqual(
            first,
            second,
            "A closed prior ticket must not silently absorb a genuinely new later flag.",
        )

    def test_report_ticket_for_recording_rejects_ticket_type_and_priority_in_vals(self):
        Ticket = self.env["hams_helpdesk.ticket"]
        with self.assertRaises(UserError):
            Ticket._ncmec_report_ticket_for_recording(
                "55555555-5555-5555-5555-555555555555",
                {"name": "x", "ticket_type": "general"},
            )

    # ------------------------------------------------------------------
    # Legal hold best-effort
    # ------------------------------------------------------------------

    def test_legal_hold_best_effort_noops_when_recording_model_not_installed(self):
        # Tests [@ANCHOR: hams_helpdesk:COMM_ncmec_apply_recording_legal_hold_best_effort]
        # ham_communications_consent (hams_com) is genuinely not installed in this hams_open-
        # only test run -- no patching needed to prove this branch, it's the real behavior.
        self.assertNotIn(_QSO_RECORDING_MODEL, self.env)
        ticket = self._create_csam_ticket(
            ncmec_recording_uuid="66666666-6666-6666-6666-666666666666"
        )
        self.assertFalse(ticket.ncmec_legal_hold_applied)
        self.assertIn("not installed", ticket.ncmec_legal_hold_note)

    def test_legal_hold_best_effort_noops_with_no_recording_uuid(self):
        ticket = self._create_csam_ticket()
        self.assertFalse(ticket.ncmec_legal_hold_applied)
        self.assertFalse(ticket.ncmec_legal_hold_note)

    def test_legal_hold_best_effort_records_access_error_gracefully(self):
        # Tests [@ANCHOR: hams_helpdesk:COMM_ncmec_recording_model_installed]
        Ticket = type(self.env["hams_helpdesk.ticket"])
        self.safe_patch_object(
            Ticket, "_ncmec_recording_model_installed", lambda self: True, create=True
        )

        def _raise(self, model_name):
            raise AccessError(_("simulated: base.group_system required"))

        self.safe_patch_object(
            Ticket, "_ncmec_attempt_legal_hold_call", _raise, create=True
        )
        ticket = self._create_csam_ticket(
            ncmec_recording_uuid="77777777-7777-7777-7777-777777777777"
        )
        self.assertFalse(ticket.ncmec_legal_hold_applied)
        self.assertIn("Automatic legal hold failed", ticket.ncmec_legal_hold_note)

    def test_legal_hold_best_effort_succeeds_when_call_succeeds(self):
        Ticket = type(self.env["hams_helpdesk.ticket"])
        self.safe_patch_object(
            Ticket, "_ncmec_recording_model_installed", lambda self: True, create=True
        )
        self.safe_patch_object(
            Ticket, "_ncmec_attempt_legal_hold_call", lambda self, model_name: True, create=True
        )
        ticket = self._create_csam_ticket(
            ncmec_recording_uuid="88888888-8888-8888-8888-888888888888"
        )
        self.assertTrue(ticket.ncmec_legal_hold_applied)
        self.assertFalse(ticket.ncmec_legal_hold_note)

    def test_legal_hold_best_effort_notes_when_no_recording_row_found(self):
        Ticket = type(self.env["hams_helpdesk.ticket"])
        self.safe_patch_object(
            Ticket, "_ncmec_recording_model_installed", lambda self: True, create=True
        )
        self.safe_patch_object(
            Ticket, "_ncmec_attempt_legal_hold_call", lambda self, model_name: False, create=True
        )
        ticket = self._create_csam_ticket(
            ncmec_recording_uuid="99999999-9999-9999-9999-999999999999"
        )
        self.assertFalse(ticket.ncmec_legal_hold_applied)
        self.assertIn("No QSO recording row found", ticket.ncmec_legal_hold_note)

    def test_action_apply_legal_hold_manually_requires_group_system(self):
        # Tests [@ANCHOR: hams_helpdesk:COMM_ncmec_action_apply_legal_hold_manually]
        ticket = self._create_csam_ticket(
            ncmec_recording_uuid="aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"
        )
        with self.assertRaises(AccessError):
            ticket.with_user(self.manager_user).action_ncmec_apply_legal_hold_manually()

    def test_action_apply_legal_hold_manually_refuses_a_non_csam_ticket(self):
        ticket = self.env["hams_helpdesk.ticket"].create({"name": "Ordinary ticket"})
        with self.assertRaises(UserError):
            ticket.with_user(self.system_user).action_ncmec_apply_legal_hold_manually()

    def test_action_apply_legal_hold_manually_surfaces_the_failure_reason(self):
        Ticket = type(self.env["hams_helpdesk.ticket"])
        self.safe_patch_object(
            Ticket, "_ncmec_recording_model_installed", lambda self: True, create=True
        )

        def _raise(self, model_name):
            raise AccessError(_("simulated"))

        self.safe_patch_object(
            Ticket, "_ncmec_attempt_legal_hold_call", _raise, create=True
        )
        ticket = self._create_csam_ticket(
            ncmec_recording_uuid="bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"
        )
        with self.assertRaises(UserError):
            ticket.with_user(self.system_user).action_ncmec_apply_legal_hold_manually()

    # ------------------------------------------------------------------
    # action_ncmec_report / manual fallback / double-file guard
    # ------------------------------------------------------------------

    def test_action_ncmec_report_requires_group_system(self):
        # Tests [@ANCHOR: hams_helpdesk:COMM_ncmec_action_report]
        ticket = self._create_csam_ticket()
        with self.assertRaises(AccessError):
            ticket.with_user(self.manager_user).action_ncmec_report()

    def test_action_ncmec_report_refuses_a_non_csam_ticket(self):
        ticket = self.env["hams_helpdesk.ticket"].create({"name": "Ordinary ticket"})
        with self.assertRaises(UserError):
            ticket.with_user(self.system_user).action_ncmec_report()

    def test_action_ncmec_report_without_credentials_explains_manual_fallback(self):
        ticket = self._create_csam_ticket()
        # hams_helpdesk.ncmec_api_base_url is unset by default (never seeded by this module's
        # own data XML, on purpose) -- the real, no-patching-needed behavior.
        with self.assertRaises(UserError) as cm:
            ticket.with_user(self.system_user).action_ncmec_report()
        self.assertIn("NCMEC's own CyberTipline reporting portal", str(cm.exception))
        self.assertEqual(ticket.ncmec_report_state, "not_reported")

    def test_action_ncmec_report_refuses_a_second_submission(self):
        ticket = self._create_csam_ticket()
        ticket.write({"ncmec_report_state": "report_submitted"})
        with self.assertRaises(UserError):
            ticket.with_user(self.system_user).action_ncmec_report()

    def test_action_ncmec_report_submits_via_the_real_api_when_configured(self):
        # Tests [@ANCHOR: hams_helpdesk:COMM_ncmec_submit_report_via_api]
        utils = self.env["zero_sudo.security.utils"]
        self.safe_patch_object(
            type(utils),
            "_get_system_param",
            lambda self, key, default=None: {
                "hams_helpdesk.ncmec_api_base_url": _FAKE_NCMEC_BASE_URL,
                "hams_helpdesk.ncmec_api_username": "test_user",
                "hams_helpdesk.ncmec_api_password": "test_pass",
                "hams_helpdesk.ncmec_contact_email": "admin@hams.com",
                "hams_helpdesk.ncmec_contact_phone": "+1 510-473-7367",
                "web.base.url": "https://hams.com",
            }.get(key, default),
            create=True,
        )
        mock_response = MagicMock()
        mock_response.read.return_value = (
            b"<reportResponse><reportId>NCMEC-TEST-123</reportId></reportResponse>"
        )
        mock_response.__enter__.return_value = mock_response
        mock_urlopen = self.safe_patch(
            "odoo.addons.hams_helpdesk.models.helpdesk_ticket._urlopen_ssrf_safe",
            return_value=mock_response,
        )

        ticket = self._create_csam_ticket()
        ticket.with_user(self.system_user).action_ncmec_report()

        self.assertEqual(ticket.ncmec_report_state, "report_submitted")
        self.assertEqual(ticket.ncmec_report_reference, "NCMEC-TEST-123")
        self.assertEqual(ticket.ncmec_report_submitted_by_id, self.system_user)
        self.assertTrue(ticket.ncmec_report_submitted_at)

        # The mocked call must have gone to the fake, obviously-not-real base URL, never a
        # hardcoded real NCMEC host, and used HTTP Basic Auth as NCMEC's own documentation
        # requires -- and through the SSRF-safe fetch helper, not a bare urlopen()/requests call.
        mock_urlopen.assert_called_once()
        called_request = mock_urlopen.call_args.args[0]
        self.assertTrue(called_request.full_url.startswith(_FAKE_NCMEC_BASE_URL))
        self.assertTrue(called_request.get_header("Authorization", "").startswith("Basic "))
        self.assertTrue(mock_urlopen.call_args.kwargs.get("https_only"))

    def test_action_ncmec_report_surfaces_an_http_error_without_changing_state(self):
        utils = self.env["zero_sudo.security.utils"]
        self.safe_patch_object(
            type(utils),
            "_get_system_param",
            lambda self, key, default=None: {
                "hams_helpdesk.ncmec_api_base_url": _FAKE_NCMEC_BASE_URL,
                "hams_helpdesk.ncmec_api_username": "test_user",
                "hams_helpdesk.ncmec_api_password": "test_pass",
            }.get(key, default),
            create=True,
        )
        self.safe_patch(
            "odoo.addons.hams_helpdesk.models.helpdesk_ticket._urlopen_ssrf_safe",
            side_effect=urllib.error.HTTPError(
                f"{_FAKE_NCMEC_BASE_URL}/submit",
                401,
                "Unauthorized",
                None,
                io.BytesIO(b"unauthorized"),
            ),
        )

        ticket = self._create_csam_ticket()
        with self.assertRaises(UserError):
            ticket.with_user(self.system_user).action_ncmec_report()
        self.assertEqual(ticket.ncmec_report_state, "not_reported")

    def test_action_ncmec_report_surfaces_an_ssrf_validation_error_without_changing_state(self):
        utils = self.env["zero_sudo.security.utils"]
        self.safe_patch_object(
            type(utils),
            "_get_system_param",
            lambda self, key, default=None: {
                "hams_helpdesk.ncmec_api_base_url": _FAKE_NCMEC_BASE_URL,
                "hams_helpdesk.ncmec_api_username": "test_user",
                "hams_helpdesk.ncmec_api_password": "test_pass",
            }.get(key, default),
            create=True,
        )
        self.safe_patch(
            "odoo.addons.hams_helpdesk.models.helpdesk_ticket._urlopen_ssrf_safe",
            side_effect=SSRFValidationError("hostname does not resolve"),
        )

        ticket = self._create_csam_ticket()
        with self.assertRaises(UserError):
            ticket.with_user(self.system_user).action_ncmec_report()
        self.assertEqual(ticket.ncmec_report_state, "not_reported")

    def test_action_mark_report_filed_manually_requires_group_system(self):
        # Tests [@ANCHOR: hams_helpdesk:COMM_ncmec_action_mark_report_filed_manually]
        ticket = self._create_csam_ticket()
        with self.assertRaises(AccessError):
            ticket.with_user(self.manager_user).action_ncmec_mark_report_filed_manually()

    def test_action_mark_report_filed_manually_records_the_filing(self):
        ticket = self._create_csam_ticket()
        ticket.with_user(self.system_user).action_ncmec_mark_report_filed_manually()
        self.assertEqual(ticket.ncmec_report_state, "report_submitted")
        self.assertEqual(ticket.ncmec_report_submitted_by_id, self.system_user)
        self.assertTrue(ticket.ncmec_report_submitted_at)

    def test_action_mark_report_filed_manually_refuses_a_second_submission(self):
        ticket = self._create_csam_ticket()
        ticket.with_user(self.system_user).action_ncmec_mark_report_filed_manually()
        with self.assertRaises(UserError):
            ticket.with_user(self.system_user).action_ncmec_mark_report_filed_manually()

    # ------------------------------------------------------------------
    # ticket_type selection survives (matches ham_repeater_dir's own established test shape
    # for extending this exact field)
    # ------------------------------------------------------------------

    def test_csam_ticket_type_is_a_real_selection_value(self):
        values = dict(self.env["hams_helpdesk.ticket"]._fields["ticket_type"].selection)
        self.assertIn(_CSAM_TICKET_TYPE, values)
        self.assertIn("general", values, "hams_helpdesk's own base value must survive")
