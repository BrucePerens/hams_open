# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""The reporting member(s) of an automatic exception ticket (models/exception_member.py).

Identity is LINKED, never copied: a many2one to res.users per (fingerprint, member), with the name, callsign(s)
and email computed live for helpdesk staff. These tests prove who is linked (and who never is), that nothing
personal reaches the ticket text, a sample, a log line, a message or a mail, that the details follow the member's
own records (change, archive, delete), that only staff can read them, and that the reply is a draft.
"""
from odoo import fields
from odoo.exceptions import AccessError, UserError
from odoo.tests.common import tagged

from odoo.addons.hams_helpdesk.models import exception_fingerprint as reporter
from odoo.addons.hams_helpdesk.models import exception_member as member_model

from .test_exception_tickets import ExceptionTicketCase, raise_attribute_error

NAME = "Marguerite Quillfeather"
EMAIL = "marguerite.quillfeather@example.net"
LOGIN = "exc_member_marguerite"


class MemberCase(ExceptionTicketCase):
    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.Member = cls.env["hams_helpdesk.exception.member"]
        cls.callsign = cls.get_callsign("W6MEMBR")
        cls.second_callsign = cls.get_callsign("W6SECND")
        cls.member = cls.env["res.users"].create(
            {
                "name": NAME,
                "login": LOGIN,
                "email": EMAIL,
                "callsign": cls.callsign,
                "group_ids": [(6, 0, [cls.env.ref("base.group_portal").id])],
            }
        )
        cls.staff = cls.env["res.users"].create(
            {
                "name": "Exception Member Staff",
                "login": "exc_member_staff",
                # A manager, who sees every ticket; a plain helpdesk user sees only unassigned or own tickets.
                "group_ids": [(6, 0, [cls.env.ref("hams_helpdesk.group_helpdesk_manager").id])],
            }
        )
        cls.portal_other = cls.env["res.users"].create(
            {
                "name": "Exception Member Portal",
                "login": "exc_member_portal_other",
                "group_ids": [(6, 0, [cls.env.ref("base.group_portal").id])],
            }
        )

    def fail_as(self, user, source="http", path="/page", **kwargs):
        """Reports one failure on ``path`` as ``user`` (a res.users record or None) and returns the fingerprint."""
        uid = user.id if user else None
        self.assertTrue(
            self.report(
                self.caught(raise_attribute_error), source, uid=uid, logged_in=bool(user), path=path, **kwargs
            )
        )
        return self.fingerprints().filtered(lambda row: row.route == path)

    def forget_gate(self):
        reporter._GATE.clear()
        reporter._RECENT_WRITES.clear()

    def rewind(self, fingerprint):
        """Makes the fingerprint's last write five minutes old, so the next report is written."""
        service = self.env["zero_sudo.security.utils"]._get_service_uid(reporter.SERVICE_XMLID)
        fingerprint.with_user(service).write(
            {"last_written_at": fields.Datetime.subtract(fields.Datetime.now(), minutes=5)}
        )
        self.forget_gate()

    def the_panel(self, fingerprint, as_user=None):
        """The member rows of the fingerprint's ticket as a staff member sees them."""
        rows = fingerprint.ticket_id.with_user(as_user or self.staff).affected_member_ids
        return rows


@tagged("post_install", "-at_install", "standard")
class TestWhoIsLinked(MemberCase):
    # Tests [@ANCHOR: hams_helpdesk:exc_reportable_member]
    # Tests [@ANCHOR: hams_helpdesk:exc_note_member]
    # Tests [@ANCHOR: hams_helpdesk:exc_record_occurrence]

    def test_a_signed_in_member_is_linked_to_the_fingerprint_and_the_ticket(self):
        row = self.fail_as(self.member)
        self.assertEqual(row.member_ids.user_id, self.member)
        self.assertEqual(row.last_member_id, self.member)
        self.assertEqual(row.member_ids.occurrence_count, 1)
        self.assertEqual(row.member_ids.ticket_id, row.ticket_id)
        self.assertEqual(self.the_panel(row), row.member_ids)

    def test_nobody_is_linked_for_a_logged_out_visitor_a_service_account_odoobot_or_an_archived_account(self):
        archived = self.env["res.users"].create({"name": "Archived Member", "login": "exc_member_archived"})
        archived.action_archive()
        service = self.env["zero_sudo.security.utils"]._get_service_env(reporter.SERVICE_XMLID).user
        refused = {
            "a logged-out visitor": None,
            "the public user": self.env.ref("base.public_user"),
            "OdooBot": self.env.ref("base.user_root"),
            "a service account": service,
            "an archived account": archived,
        }
        for index, (label, user) in enumerate(refused.items()):
            self.forget_gate()
            row = self.fail_as(user, path=f"/refused{index}")
            self.assertFalse(row.member_ids, label)
            self.assertFalse(row.last_member_id, label)

    def test_only_sources_with_a_person_behind_them_link_anyone(self):
        self.env.flush_all()
        for index, source in enumerate(("mail", "daemon")):
            self.forget_gate()
            path = f"/job{index}"
            self.assertTrue(
                reporter.report_event(
                    self.env.registry, source, "x.Err", "mod", "frame", path, "trace", "name", uid=self.member.id
                )
            )
            self.env.invalidate_all()
            row = self.fingerprints().filtered(lambda r: r.route == path)
            self.assertTrue(row)
            self.assertFalse(row.member_ids, f"a {source} failure is not a member's")
        self.forget_gate()
        self.assertFalse(self.fail_as(self.member, source="cron", path="/cron").member_ids)
        for index, source in enumerate(("http", "rpc")):
            self.forget_gate()
            self.assertTrue(self.fail_as(self.member, source=source, path=f"/page{index}").member_ids)

    def test_the_same_member_again_is_counted_not_listed_twice(self):
        row = self.fail_as(self.member)
        self.rewind(row)
        self.fail_as(self.member)
        row = self.fingerprints().filtered(lambda r: r.route == "/page")
        self.assertEqual(len(row.member_ids), 1)
        self.assertEqual(row.member_ids.occurrence_count, 2)
        self.assertGreaterEqual(row.member_ids.last_seen, row.member_ids.first_seen)

    def test_a_second_member_inside_the_one_minute_gate_is_still_listed(self):
        first = self.fail_as(self.member)
        again = self.fail_as(self.portal_other)
        self.assertEqual(first, again)
        self.assertEqual(again.member_ids.user_id, self.member | self.portal_other)
        self.assertEqual(again.occurrence_count, 2)

    def test_the_list_is_capped_and_the_overflow_is_only_counted(self):
        self.safe_patch_object(member_model, "MEMBER_LIST_LIMIT", new=1)
        row = self.fail_as(self.member)
        self.rewind(row)
        self.fail_as(self.portal_other)
        row = self.fingerprints().filtered(lambda r: r.route == "/page")
        self.assertEqual(row.member_ids.user_id, self.member)
        self.assertEqual(row.unlisted_member_count, 1)

    def test_the_fingerprint_is_the_same_for_every_member(self):
        first = self.fail_as(self.member)
        second = self.fail_as(self.portal_other)
        self.assertEqual(len(self.fingerprints()), 1)
        self.assertEqual(first.fingerprint, second.fingerprint)


@tagged("post_install", "-at_install", "standard")
class TestPanelDetails(MemberCase):
    # Tests [@ANCHOR: hams_helpdesk:ticket_affected_members]
    # Tests [@ANCHOR: hams_helpdesk:exc_contact_user]

    def panel_row(self):
        return self.the_panel(self.fail_as(self.member))

    def test_staff_see_name_callsign_and_email_computed_from_the_member(self):
        panel = self.panel_row()
        self.assertEqual(panel.member_name, NAME)
        self.assertEqual(panel.callsigns, self.callsign.upper())
        self.assertEqual(panel.contact_kind, "member")
        self.assertEqual(panel.contact_name, NAME)
        self.assertEqual(panel.contact_email, EMAIL)

    def test_a_member_with_no_callsign_still_shows_name_and_email(self):
        no_call = self.env["res.users"].create(
            {"name": "W6MSU", "login": "exc_member_w6msu", "email": "w6msu@example.net"}
        )
        panel = self.the_panel(self.fail_as(no_call, path="/nocall"))
        self.assertFalse(panel.callsigns)
        self.assertEqual(panel.member_name, "W6MSU")
        self.assertEqual(panel.contact_email, "w6msu@example.net")

    def test_callsigns_come_from_the_account_and_from_linked_callbook_rows(self):
        self.env["ham.callbook"].create({"callsign": self.second_callsign, "user_id": self.member.id})
        self.env.flush_all()
        panel = self.panel_row()
        self.assertEqual(
            sorted(panel.callsigns.split(", ")), sorted([self.callsign.upper(), self.second_callsign.upper()])
        )

    def test_the_details_follow_the_members_own_records_not_a_copy(self):
        row = self.fail_as(self.member)
        self.member.write({"name": "Renamed Member", "email": "renamed@example.net"})
        self.env.flush_all()
        self.env.invalidate_all()
        panel = self.the_panel(row)
        self.assertEqual((panel.member_name, panel.contact_email), ("Renamed Member", "renamed@example.net"))

    def test_an_archived_or_erased_account_shows_nothing_about_the_member(self):
        row = self.fail_as(self.member)
        self.member.action_archive()
        self.env.flush_all()
        self.env.invalidate_all()
        panel = self.the_panel(row)
        self.assertEqual(panel.contact_kind, "closed")
        for field in ("member_name", "callsigns", "contact_name", "contact_email"):
            self.assertFalse(panel[field], field)
        with self.assertRaises(UserError):
            panel.action_reply_to_member()

    def test_deleting_the_account_keeps_the_row_and_drops_the_link(self):
        row = self.fail_as(self.member)
        member_row = row.member_ids
        self.env.flush_all()
        self.member.unlink()
        self.env.invalidate_all()
        self.assertTrue(member_row.exists())
        self.assertFalse(member_row.user_id)
        self.assertFalse(row.last_member_id)
        panel = self.the_panel(row)
        self.assertEqual(panel.contact_kind, "closed")
        self.assertFalse(panel.member_name)
        self.assertFalse(panel.contact_email)

    def test_the_ticket_list_gets_the_member_count_and_the_latest_member_name(self):
        row = self.fail_as(self.member)
        self.fail_as(self.portal_other)
        ticket = row.ticket_id.with_user(self.staff)
        self.assertEqual(ticket.affected_member_count, 2)
        self.assertEqual(ticket.latest_member_label, "Exception Member Portal")

    def test_a_ticket_with_no_member_has_a_zero_count_and_no_name(self):
        row = self.fail_as(None)
        ticket = row.ticket_id.with_user(self.staff)
        self.assertEqual(ticket.affected_member_count, 0)
        self.assertFalse(ticket.latest_member_label)

    def test_the_base_hook_writes_to_the_member_directly(self):
        self.assertEqual(self.Member._contact_user(self.member), ("member", self.member))


@tagged("post_install", "-at_install", "standard")
class TestReplyIsOnlyADraft(MemberCase):
    # Tests [@ANCHOR: hams_helpdesk:exc_reply_to_member]

    def test_reply_opens_the_composer_addressed_to_the_member_and_sends_nothing(self):
        panel = self.the_panel(self.fail_as(self.member))
        ticket = panel.ticket_id
        messages = self.env["mail.message"].search_count([])
        mails = self.env["mail.mail"].search_count([])
        action = panel.action_reply_to_member()
        context = action["context"]
        self.assertEqual(action["res_model"], "mail.compose.message")
        self.assertEqual(action["target"], "new")
        self.assertEqual(context["default_partner_ids"], self.member.partner_id.ids)
        self.assertEqual(context["default_res_ids"], ticket.ids)
        self.assertFalse(context["mail_post_autofollow"])
        self.assertIn("working on", context["default_body"])
        for leaked in (NAME, EMAIL, self.callsign, "Traceback", "/page"):
            self.assertNotIn(leaked, context["default_body"] + context["default_subject"], leaked)
        self.assertEqual(self.env["mail.message"].search_count([]), messages, "nothing was posted")
        self.assertEqual(self.env["mail.mail"].search_count([]), mails, "nothing was queued")
        self.assertNotIn(self.member.partner_id, ticket.message_partner_ids, "the member is not made a follower")

    def test_the_mailto_action_is_a_plain_mailto_link(self):
        panel = self.the_panel(self.fail_as(self.member))
        action = panel.action_mailto()
        self.assertEqual(action["type"], "ir.actions.act_url")
        self.assertTrue(action["url"].startswith("mailto:" + EMAIL + "?subject="))


@tagged("post_install", "-at_install", "standard")
class TestStaffOnly(MemberCase):
    # Tests [@ANCHOR: hams_helpdesk:ticket_member_panel]

    def test_staff_can_read_and_portal_users_cannot(self):
        row = self.fail_as(self.member)
        ticket = row.ticket_id
        self.assertEqual(len(ticket.with_user(self.staff).affected_member_ids), 1)
        # A portal member reads their own ticket; the panel's fields are not among what they get.
        own = self.Ticket.create({"name": "Mine", "partner_id": self.portal_other.partner_id.id})
        as_portal = own.with_user(self.portal_other)
        self.assertEqual(as_portal.name, "Mine")
        for field in ("affected_member_ids", "affected_member_count", "latest_member_label"):
            self.assertNotIn(field, as_portal.fields_get(), field)
            self.assertNotIn(field, as_portal.read()[0], field)
            with self.assertRaises(AccessError):
                as_portal.mapped(field)
        self.assertFalse(self.Ticket.with_user(self.portal_other).search([("id", "=", ticket.id)]))
        with self.assertRaises(AccessError):
            self.Member.with_user(self.portal_other).search([])

    def test_the_member_cannot_read_their_own_row_or_the_ticket(self):
        row = self.fail_as(self.member)
        self.assertFalse(self.Ticket.with_user(self.member).search([("id", "=", row.ticket_id.id)]))

    def test_staff_cannot_change_the_rows(self):
        panel = self.the_panel(self.fail_as(self.member))
        with self.assertRaises(AccessError):
            panel.write({"occurrence_count": 99})
        with self.assertRaises(AccessError):
            panel.unlink()

    def test_the_form_and_list_views_carry_the_panel_for_staff_only(self):
        form = self.Ticket.with_user(self.staff).get_view(self.env.ref("hams_helpdesk.view_hams_helpdesk_ticket_form").id)
        self.assertIn("affected_member_ids", form["arch"])
        self.assertIn("action_reply_to_member", form["arch"])
        form = self.Ticket.with_user(self.portal_other).get_view(self.env.ref("hams_helpdesk.view_hams_helpdesk_ticket_form").id)
        self.assertNotIn("affected_member_ids", form["arch"])
        self.assertNotIn("action_reply_to_member", form["arch"])
        lists = self.Ticket.with_user(self.staff).get_view(self.env.ref("hams_helpdesk.view_hams_helpdesk_ticket_list").id)
        self.assertIn("affected_member_count", lists["arch"])


@tagged("post_install", "-at_install", "standard")
class TestNoIdentityLeavesTheRecord(MemberCase):
    """The name, email and callsign are only ever computed for staff on screen. They are in no text, sample,
    message, mail, log line or fingerprint, so they cannot reach PagerDuty, the AI triage reader, an outbound
    email or the exception-report unit."""

    # Tests [@ANCHOR: hams_helpdesk:exc_ticket_body]

    def identity(self):
        return (NAME, "Quillfeather", EMAIL, "quillfeather", LOGIN, self.callsign, self.callsign.lower())

    def test_nothing_personal_is_written_anywhere_a_report_goes(self):
        messages = self.env["mail.message"].search([]).ids
        mails = self.env["mail.mail"].search([]).ids
        with self.assertLogs("hams_helpdesk.exception_reporter", level="DEBUG") as logs:
            reporter._reporter_logger.debug("sentinel")
            row = self.fail_as(self.member)
            self.fail_as(self.member, source="rpc", path="/other")
        ticket = row.ticket_id
        texts = [ticket.name, str(ticket.description), row.name, row.fingerprint, row.route, row.frame,
                 row.samples_json, row.reporters_json, row.exc_type, row.module]
        texts += [m.body or "" for m in ticket.message_ids] + [m.subject or "" for m in ticket.message_ids]
        texts += [str(m.email_to) + str(m.body_html) + str(m.subject) for m in self.env["mail.mail"].search([("id", "not in", mails)])]
        texts += [m.body or "" for m in self.env["mail.message"].search([("id", "not in", messages)])]
        texts += [r.getMessage() for r in logs.records]
        blob = "\n".join(texts)
        for leaked in self.identity():
            self.assertNotIn(leaked, blob, leaked)
        # What was linked is the account id, an integer, in the samples.
        self.assertIn(str(self.member.id), row.samples_json)

    def test_the_outbound_service_accounts_cannot_read_the_panel(self):
        row = self.fail_as(self.member)
        ticket = row.ticket_id
        utils = self.env["zero_sudo.security.utils"]
        for xmlid in (
            reporter.SERVICE_XMLID,
            "hams_helpdesk.user_ai_triage_service",
            "hams_helpdesk.user_github_webhook_service",
        ):
            env = utils._get_service_env(xmlid)
            readable = env["hams_helpdesk.ticket"].browse(ticket.id)
            if not readable.exists():
                continue
            self.assertNotIn("affected_member_ids", readable.fields_get(), xmlid)
            with self.assertRaises(AccessError):
                readable.mapped("affected_member_ids")

    def test_the_description_says_what_is_true_for_each_source(self):
        member_row = self.fail_as(self.member)
        text = str(member_row.ticket_id.description)
        self.assertIn("the member's account is linked to this report", text)
        self.assertIn("Request bodies, form values, page text and query strings are never recorded", text)
        self.assertNotIn("Personal data, query strings", text)
        self.forget_gate()
        cron_row = self.fail_as(None, source="cron", path="/job")
        cron_text = str(cron_row.ticket_id.description)
        self.assertIn("No member's account is linked to this report", cron_text)
        self.assertNotIn("is linked to this report (staff", cron_text)

    def test_the_fingerprint_row_and_ticket_hold_links_only(self):
        row = self.fail_as(self.member)
        stored = {name: row[name] for name in row._fields if name in ("name", "route", "frame", "fingerprint")}
        self.assertNotIn(self.member.name, " ".join(map(str, stored.values())))
        self.assertEqual(row.last_member_id, self.member)
        self.assertEqual(type(row.last_member_id.id), int)
