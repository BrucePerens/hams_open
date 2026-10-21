# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""One failing systemd unit is ONE ticket, however many things report it.

On 2026-10-21 a single failure of fcc.uls.sync.service (FCC answered 403) produced four tickets: the
OnFailure exception report and three pager checks, each of the pager ones titled "[PAGER] INC-AUTO".
Every report about a unit now lands on the unit's exception fingerprint: one row, one ticket, an
occurrence count, first and last seen and the list of reporters.
"""
import json
from datetime import timedelta

from odoo import fields
from odoo.tests.common import tagged

from odoo.addons.hams_helpdesk.models import exception_fingerprint as reporter
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

FCC = "fcc.uls.sync.service"
TRACKER = "Systemd Failed Services Tracker"
SERVICES = "Systemd Services (FCC sync)"
FRESH = "Freshness: scheduled syncs"
ETC_FCC_MESSAGES = {
    TRACKER: "1 failed systemd unit(s): fcc.uls.sync.service (exit-code since 2026-10-21 05:19:35 UTC). "
    "Clear one that is understood and fixed with `systemctl reset-failed <unit>`.",
    SERVICES: "Systemd services not active: fcc.uls.sync.service (failed)",
    FRESH: "1 of 20 units are stale: The last run of fcc.uls.sync.service did not succeed (result: exit-code)",
}


@tagged("post_install", "-at_install", "standard")
class TestUnitTicketDedup(HamsTransactionCase):
    # Tests [@ANCHOR: pager_duty:unit_ticket_join]
    # Tests [@ANCHOR: hams_helpdesk:exc_report_unit_failures]
    # Tests [@ANCHOR: hams_helpdesk:exc_unit_key_parts]
    # Tests [@ANCHOR: hams_helpdesk:exc_reporters]
    # Tests [@ANCHOR: report_incident_rate_limit]

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.Incident = cls.env["pager.incident"]
        cls.Fingerprint = cls.env["hams_helpdesk.exception.fingerprint"]
        cls.Ticket = cls.env["hams_helpdesk.ticket"]

    def setUp(self):
        super().setUp()
        # The reporter writes on its own cursor, which joins this test's transaction in registry test mode.
        registry_test_mode = self.enter_registry_test_mode()
        registry_test_mode.__enter__()
        self.addCleanup(registry_test_mode.__exit__, None, None, None)
        reporter._GATE.clear()
        reporter._RECENT_WRITES.clear()
        self.addCleanup(reporter._GATE.clear)
        self.addCleanup(reporter._RECENT_WRITES.clear)
        # Not under test here: the shared Redis rate limit would make a rerun within a minute return False.
        self.safe_patch("odoo.addons.pager_duty.models.incident.redis_pool", None)
        self.marker = {"value": "A"}
        self.safe_patch_object(
            type(self.Fingerprint), "_release_marker", side_effect=lambda *args: self.marker["value"]
        )

    def as_exception_reporter(self):
        uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "hams_helpdesk.user_exception_reporter_service"
        )
        return self.Fingerprint.with_user(uid)

    def exception_report(self, unit, kind="unit_failed", result="exit-code"):
        self.env.flush_all()
        answer = self.as_exception_reporter().report_daemon_failure(unit, kind, result, "boom")
        self.env.invalidate_all()
        return answer

    def pager_report(self, source, units, message=None, severity="high"):
        """What generalized_monitor's report() sends, through the real report_incident."""
        self.env.flush_all()
        incident_id = self.Incident.report_incident(
            {
                "source": source,
                "severity": severity,
                "description": message or ETC_FCC_MESSAGES.get(source) or f"{', '.join(units)} failing",
                "units": list(units),
            }
        )
        self.env.invalidate_all()
        return self.Incident.browse(incident_id) if incident_id else self.Incident

    def rows(self):
        return self.Fingerprint.search([("source", "=", "daemon")])

    def row_for(self, unit):
        return self.rows().filtered(lambda row: row.module == unit)

    def ticket_titles(self):
        return self.Ticket.search([]).mapped("name")

    def make_repeat_due(self):
        """Lets the next report through the one-minute gate, as a minute passing would."""
        self.rows().write({"last_written_at": fields.Datetime.now() - timedelta(seconds=120)})
        reporter._GATE.clear()

    def test_the_fcc_failure_is_one_ticket_with_a_count_of_four(self):
        """The exact 2026-10-21 scenario: the exception report, then the three pager checks."""
        self.assertEqual(self.exception_report(FCC), {"recorded": True})
        incidents = [self.pager_report(source, [FCC]) for source in (TRACKER, SERVICES, FRESH)]
        row = self.row_for(FCC)
        self.assertEqual(len(row), 1)
        self.assertEqual(row.occurrence_count, 4)
        self.assertEqual(
            row._reporters(), ["exception report (unit_failed)", TRACKER, SERVICES, FRESH]
        )
        tickets = row.ticket_id
        self.assertEqual(len(tickets), 1)
        self.assertEqual(tickets.name, f"Server exception: Daemon failure: {FCC}")
        self.assertEqual(len(self.Ticket.search([("name", "ilike", FCC)])), 1)
        for incident in incidents:
            self.assertEqual(incident.helpdesk_ticket_id, tickets.id)
            self.assertEqual(incident.helpdesk_ticket_model, "hams_helpdesk.ticket")
        # No fourth ticket, and no placeholder title anywhere.
        self.assertFalse([name for name in self.ticket_titles() if "INC-AUTO" in name or "[PAGER]" in name])
        self.assertNotIn("INC-AUTO", " ".join(self.Incident.search([]).mapped("name")))
        self.assertIn("Reported by", tickets.description)
        self.assertIn(TRACKER, tickets.description)
        self.assertIn("Occurrences", tickets.description)
        self.assertTrue(row.first_seen and row.last_seen)

    def test_the_order_does_not_matter_the_pager_check_may_report_first(self):
        self.pager_report(TRACKER, [FCC])
        self.exception_report(FCC)
        self.pager_report(FRESH, [FCC])
        row = self.row_for(FCC)
        self.assertEqual(row.occurrence_count, 3)
        self.assertEqual(len(self.Ticket.search([("name", "ilike", FCC)])), 1)

    def test_a_repeat_of_the_same_check_counts_but_makes_no_second_ticket(self):
        self.pager_report(TRACKER, [FCC])
        self.make_repeat_due()
        self.pager_report(TRACKER, [FCC])
        self.make_repeat_due()
        self.pager_report(TRACKER, [FCC])
        row = self.row_for(FCC)
        self.assertEqual(row.occurrence_count, 3)
        self.assertEqual(row._reporters(), [TRACKER])
        self.assertEqual(len(self.Ticket.search([("name", "ilike", FCC)])), 1)
        # Same minute: counted for later, not written (the exception gate).
        self.pager_report(TRACKER, [FCC])
        self.assertEqual(self.row_for(FCC).occurrence_count, 3)
        self.assertEqual(sum(entry[1] for entry in reporter._GATE.values()), 1)

    def test_every_unit_gets_exactly_one_ticket_mixed_scenario(self):
        """Three units, different mixes of reporters, plus one check that names two units."""
        a, b, c = "adif.processor.service", "club.crawl.service", "qrz.scraper.service"
        self.exception_report(a)
        self.pager_report("Systemd Services A", [a])
        self.pager_report("Freshness A", [a])
        self.pager_report("Systemd Services B", [b])
        self.pager_report("Freshness B", [b])
        self.exception_report(c, kind="consumer_exception", result="signal")
        both = self.pager_report(TRACKER, [a, b], "2 failed systemd unit(s): adif.processor.service; club.crawl.service")
        self.assertEqual(len(self.rows()), 3)
        tickets = self.rows().ticket_id
        self.assertEqual(len(tickets), 3)
        self.assertEqual(len(set(tickets.ids)), 3)
        self.assertEqual(self.row_for(a).occurrence_count, 4)
        self.assertEqual(
            self.row_for(a)._reporters(),
            ["exception report (unit_failed)", "Systemd Services A", "Freshness A", TRACKER],
        )
        self.assertEqual(self.row_for(b).occurrence_count, 3)
        self.assertEqual(self.row_for(b)._reporters(), ["Systemd Services B", "Freshness B", TRACKER])
        self.assertEqual(self.row_for(c).occurrence_count, 1)
        self.assertEqual(self.row_for(c)._reporters(), ["exception report (consumer_exception)"])
        # The check that named two units links the first and says so on the incident.
        self.assertEqual(both.helpdesk_ticket_id, self.row_for(a).ticket_id.id)
        note = both.message_ids[0].body
        self.assertIn(f"#{self.row_for(a).ticket_id.id} ({a})", note)
        self.assertIn(f"#{self.row_for(b).ticket_id.id} ({b})", note)
        self.assertEqual(len(self.Ticket.search([("ticket_type", "=", "server_exception")])), 3)
        self.assertFalse([name for name in self.ticket_titles() if "INC-AUTO" in name or "[PAGER]" in name])

    def test_one_ticket_whatever_the_kind_or_result_of_the_exception_report(self):
        self.exception_report(FCC, "unit_failed", "exit-code")
        self.make_repeat_due()
        self.exception_report(FCC, "unit_failed", "signal")
        self.make_repeat_due()
        self.exception_report(FCC, "consumer_exception", "exit-code")
        row = self.row_for(FCC)
        self.assertEqual(len(row), 1)
        self.assertEqual(row.occurrence_count, 3)
        self.assertEqual(
            row._reporters(),
            ["exception report (unit_failed)", "exception report (consumer_exception)"],
        )
        self.assertEqual(len(self.Ticket.search([("ticket_type", "=", "server_exception")])), 1)

    def test_a_unit_name_without_a_suffix_is_the_service(self):
        self.pager_report("Systemd Services", ["odoo"])
        self.assertEqual(self.row_for("odoo.service").occurrence_count, 1)

    def test_a_unit_that_fails_again_after_its_ticket_was_closed_always_reopens_it_with_a_note(self):
        # Tests [@ANCHOR: hams_helpdesk:exc_reopen_reason]
        self.pager_report(TRACKER, [FCC])
        ticket = self.row_for(FCC).ticket_id
        ticket.write({"stage": "closed"})
        # Same release, same day: a unit failure still reopens (once-a-minute gate applies as ever).
        self.make_repeat_due()
        self.pager_report(TRACKER, [FCC])
        self.assertEqual(ticket.stage, "new")
        self.assertIn("reopened automatically", ticket.message_ids[0].body)
        self.assertIn("unit failed again", ticket.message_ids[0].body)
        self.assertEqual(self.row_for(FCC).occurrence_count, 2)
        self.assertEqual(len(self.Ticket.search([("name", "ilike", FCC)])), 1)
        # Resolved counts as closed.
        ticket.write({"stage": "resolved"})
        self.make_repeat_due()
        self.exception_report(FCC)
        self.assertEqual(ticket.stage, "new")

    def test_a_recurrence_inside_the_gate_does_not_reopen_or_spam(self):
        self.pager_report(TRACKER, [FCC])
        ticket = self.row_for(FCC).ticket_id
        ticket.write({"stage": "closed"})
        self.pager_report(TRACKER, [FCC])
        self.assertEqual(ticket.stage, "closed")

    def test_a_resolved_incident_that_recurs_joins_the_same_ticket(self):
        first = self.pager_report(TRACKER, [FCC])
        self.Incident.auto_resolve_incidents(TRACKER)
        self.assertEqual(first.status, "resolved")
        self.make_repeat_due()
        second = self.pager_report(TRACKER, [FCC])
        self.assertNotEqual(first.id, second.id)
        self.assertEqual(second.helpdesk_ticket_id, first.helpdesk_ticket_id)
        self.assertEqual(self.row_for(FCC).occurrence_count, 2)

    def test_low_and_medium_severities_stay_tracked_not_ticketed(self):
        incident = self.pager_report("Systemd Services", [FCC], severity="low")
        self.assertFalse(self.rows())
        self.assertFalse(incident.helpdesk_ticket_id)

    def test_a_check_that_names_no_unit_keeps_its_own_ticket_titled_with_the_check(self):
        incident = self.pager_report("Disk Space (/)", [], "Disk is 97 percent full")
        ticket = self.Ticket.browse(incident.helpdesk_ticket_id)
        self.assertEqual(ticket.name, "[PAGER] Disk Space (/)")
        self.assertFalse(self.rows())

    def test_instances_of_a_template_unit_are_not_joined(self):
        """hams-exception-report@<x> failing names an instance, not a unit with a row of its own."""
        incident = self.pager_report(
            TRACKER, ["hams-exception-report@club.crawl.service.service"], "1 failed systemd unit(s): hams-exception-report@club"
        )
        self.assertFalse(self.rows())
        ticket = self.Ticket.browse(incident.helpdesk_ticket_id)
        self.assertEqual(ticket.name, f"[PAGER] {TRACKER}")

    def test_a_full_new_ticket_cap_falls_back_to_a_pager_ticket_so_the_page_is_not_lost(self):
        self.safe_patch_object(reporter, "NEW_TICKETS_PER_HOUR", new=0)
        incident = self.pager_report(TRACKER, [FCC])
        self.assertEqual(self.row_for(FCC).occurrence_count, 1)
        self.assertFalse(self.row_for(FCC).ticket_id)
        self.assertEqual(self.Ticket.browse(incident.helpdesk_ticket_id).name, f"[PAGER] {TRACKER}")

    def test_only_scrubbed_message_text_is_stored(self):
        self.pager_report(
            TRACKER, [FCC], "1 failed systemd unit(s): fcc.uls.sync.service password=hunter2 mail k6bp@example.com"
        )
        row = self.row_for(FCC)
        stored = " ".join((row.samples_json, row.ticket_id.description))
        for needle in ("hunter2", "k6bp@example.com"):
            self.assertNotIn(needle, stored)
        self.assertEqual(json.loads(row.samples_json)[-1]["source"], "daemon")
