# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""
Pushing `cloudflare.dns.record` rows to Cloudflare, from Odoo.

Bruce's rule (2026-10-04): every DNS change goes through hams.com's Odoo; Odoo is the source of truth.
Nothing here can reach the real Cloudflare API: the four functions the model calls
(`find_zone_id`, `list_dns_records_named`, `create_dns_record`, `update_dns_record`) are replaced by one
in-memory Cloudflare, and the HTTP layer underneath them is patched where it is tested directly.
The guarantees under test, in the order they matter:

* a record that already exists is adopted, never written (stun.hams.com stays as it is);
* nothing Odoo has no row for is touched, and the only delete is a row marked "retire" and linked by id;
* a plan shows exactly what an apply will do, and apply refuses a plan that is out of date.
"""
import importlib.util
import os
import uuid

from odoo.exceptions import AccessError, UserError, ValidationError
from odoo.tests.common import Form, new_test_user, tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.cloudflare.utils import cloudflare_api, dns_plan
from cryptography.fernet import Fernet

MIGRATION_PATH = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    "migrations", "1.7", "post-dns-records-observe-only.py",
)
MODEL = "odoo.addons.cloudflare.models.dns_record."
TUNNEL_CNAME = "515643a0-6c67-4c8c-9173-519a3a516fa2.cfargotunnel.com"


def _id():
    return uuid.uuid4().hex


class FakeCloudflare:
    """The slice of Cloudflare the push talks to, in memory, recording every call."""

    def __init__(self, zones):
        self.zones = {name: _id() for name in zones}
        self.records = []
        self.calls = []  # (verb, ...) in order
        self.fail_reads = False
        self.plan = ("pro", "2014-01-25T03:59:00.776074Z")
        self.count_override = None  # pretend the zone already holds this many records

    def get_zone_details(self, zone_id, token):
        self.calls.append(("zone_details", zone_id))
        if self.fail_reads:
            return False, "API Error"
        return True, {"plan": {"legacy_id": self.plan[0]}, "created_on": self.plan[1]}

    def count_dns_records(self, zone_id, token):
        self.calls.append(("count", zone_id))
        if self.fail_reads:
            return False, "API Error"
        if self.count_override is not None:
            return True, self.count_override
        return True, len([r for r in self.records if r["zone"] == zone_id])

    def add(self, zone, name, rtype, content, proxied=False, ttl=300):
        record = {"id": _id(), "zone": self.zones[zone], "name": name, "type": rtype, "content": content,
                  "proxied": proxied, "ttl": ttl}
        self.records.append(record)
        return record

    def writes(self):
        return [c for c in self.calls if c[0] in ("create", "update", "delete")]

    def find_zone_id(self, name, token):
        self.calls.append(("find_zone", name, token))
        return (False, "API Error") if self.fail_reads else (True, self.zones.get(name))

    def list_dns_records_named(self, zone_id, name, token):
        self.calls.append(("list", name))
        if self.fail_reads:
            return False, "API Error"
        return True, [dict(r) for r in self.records if r["zone"] == zone_id and r["name"] == name]

    def create_dns_record(self, zone_id, payload, token):
        self.calls.append(("create", zone_id, dict(payload)))
        record = dict(payload, id=_id(), zone=zone_id)
        self.records.append(record)
        return True, record["id"]

    def delete_dns_record(self, zone_id, record_id, token):
        self.calls.append(("delete", zone_id, record_id))
        self.records = [r for r in self.records if r["id"] != record_id]
        return True, record_id

    def update_dns_record(self, zone_id, record_id, payload, token):
        self.calls.append(("update", zone_id, record_id, dict(payload)))
        for record in self.records:
            if record["id"] == record_id:
                record.update(payload)
        return True, record_id


@tagged("post_install", "-at_install")
class TestDnsPlan(HamsTransactionCase):
    """The plan as pure data (no Odoo records, no network). Tests [@ANCHOR: cloudflare:COMM_dns_plan]"""

    def _row(self, **kw):
        row = {"id": 1, "name": "stun.hams.com", "type": "A", "content": "66.135.10.56", "proxied": False,
               "manage": True, "cf_record_id": ""}
        row.update(kw)
        return row

    def _remote(self, **kw):
        rec = {"id": _id(), "name": "stun.hams.com", "type": "A", "content": "66.135.10.56", "proxied": False, "ttl": 300}
        rec.update(kw)
        return rec

    def test_01_an_existing_identical_record_is_adopted_never_created_or_updated(self):
        # stun.hams.com must stay untouched: the first thing the planner may never do is write over it.
        remote = self._remote()
        entry = dns_plan.plan_row(self._row(), [remote])
        self.assertEqual(entry["action"], dns_plan.ADOPT)
        self.assertEqual(entry["remote_id"], remote["id"])
        self.assertNotIn(entry["action"], dns_plan.WRITING_ACTIONS)

    def test_02_adopting_refuses_when_the_proxied_flag_differs(self):
        entry = dns_plan.plan_row(self._row(proxied=True), [self._remote(proxied=False)])
        self.assertEqual(entry["action"], dns_plan.CONFLICT)

    def test_03_other_content_under_the_same_name_is_a_conflict_not_an_overwrite(self):
        entry = dns_plan.plan_row(self._row(content="192.0.2.1"), [self._remote()])
        self.assertEqual(entry["action"], dns_plan.CONFLICT)
        self.assertIn("66.135.10.56", entry["reason"])

    def test_04_a_missing_record_is_created_and_an_unmanaged_one_is_not(self):
        self.assertEqual(dns_plan.plan_row(self._row(), [])["action"], dns_plan.CREATE)
        self.assertEqual(dns_plan.plan_row(self._row(manage=False), [])["action"], dns_plan.DRIFT)

    def test_05_a_linked_row_updates_content_and_proxied_and_keeps_the_remote_ttl(self):
        remote = self._remote(content="192.0.2.9")
        entry = dns_plan.plan_row(self._row(cf_record_id=remote["id"]), [remote])
        self.assertEqual((entry["action"], entry["ttl"]), (dns_plan.UPDATE, 300))
        self.assertIn("192.0.2.9", entry["reason"])
        same = self._remote()
        self.assertEqual(dns_plan.plan_row(self._row(cf_record_id=same["id"]), [same])["action"], dns_plan.UNCHANGED)

    def test_06_a_linked_row_that_is_observe_only_reports_drift_and_writes_nothing(self):
        remote = self._remote(content="192.0.2.9")
        entry = dns_plan.plan_row(self._row(cf_record_id=remote["id"], manage=False), [remote])
        self.assertEqual(entry["action"], dns_plan.DRIFT)

    def test_07_a_linked_id_that_vanished_or_changed_type_is_a_conflict(self):
        gone = dns_plan.plan_row(self._row(cf_record_id=_id()), [self._remote()])
        self.assertEqual(gone["action"], dns_plan.CONFLICT)
        txt = self._remote(type="TXT", content="x")
        self.assertEqual(dns_plan.plan_row(self._row(cf_record_id=txt["id"]), [txt])["action"], dns_plan.CONFLICT)

    def test_08_nameserver_and_text_records_may_add_values_and_a_cname_stands_alone(self):
        existing = self._remote(name="callbook.hams.com", type="NS", content="ns1.hams.com", proxied=False)
        second = self._row(name="callbook.hams.com", type="NS", content="ns2.hams.com", proxied=False)
        entry = dns_plan.plan_row(second, [existing])
        self.assertEqual(entry["action"], dns_plan.CREATE)
        self.assertIn("next to 1 existing", entry["reason"])
        cname = self._row(name="callbook.hams.com", type="CNAME", content="x.example.com", proxied=False)
        self.assertEqual(dns_plan.plan_row(cname, [existing])["action"], dns_plan.CONFLICT)
        a_row = self._row(name="www.perens.com")
        self.assertEqual(
            dns_plan.plan_row(a_row, [self._remote(name="www.perens.com", type="CNAME", content=TUNNEL_CNAME)])["action"],
            dns_plan.CONFLICT)

    def test_09_spellings_of_one_value_compare_equal(self):
        self.assertEqual(dns_plan.normalize_content("TXT", '"v=spf1 -all"'), "v=spf1 -all")
        self.assertEqual(dns_plan.normalize_content("NS", "NS1.Hams.com."), "ns1.hams.com")
        self.assertEqual(dns_plan.normalize_content("AAAA", "2001:0db8:0000:0000:0000:0000:0000:0001"), "2001:db8::1")
        txt = self._row(type="TXT", content="v=spf1 -all", proxied=False, name="hams.com")
        entry = dns_plan.plan_row(txt, [self._remote(name="hams.com", type="TXT", content='"v=spf1 -all"')])
        self.assertEqual(entry["action"], dns_plan.ADOPT)

    def test_10_the_plan_hash_changes_with_what_would_be_written_and_the_text_says_nothing_is_deleted(self):
        entries = [dns_plan.plan_row(self._row(), [])]
        first = dns_plan.plan_hash(entries)
        entries[0]["content"] = "192.0.2.1"
        self.assertNotEqual(first, dns_plan.plan_hash(entries))
        self.assertIn("never changed or deleted", dns_plan.render(entries))

    def test_11_retire_deletes_only_a_linked_managed_record_of_the_same_type(self):
        remote = self._remote()
        entry = dns_plan.plan_row(self._row(cf_record_id=remote["id"], retire=True), [remote])
        self.assertEqual((entry["action"], entry["remote_id"]), (dns_plan.DELETE, remote["id"]))
        self.assertIn(dns_plan.DELETE, dns_plan.WRITING_ACTIONS)
        # Not linked: never deleted by name.
        self.assertEqual(dns_plan.plan_row(self._row(retire=True), [remote])["action"], dns_plan.UNCHANGED)
        # Observe only: never deleted.
        self.assertEqual(dns_plan.plan_row(self._row(cf_record_id=remote["id"], retire=True, manage=False),
                                           [remote])["action"], dns_plan.DRIFT)
        # Gone already: nothing to delete, flagged so the row can be archived.
        gone = dns_plan.plan_row(self._row(cf_record_id=_id(), retire=True), [remote])
        self.assertEqual((gone["action"], gone.get("already_gone")), (dns_plan.UNCHANGED, True))
        # A record of another type under that id is a conflict.
        txt = self._remote(type="TXT", content="x")
        self.assertEqual(dns_plan.plan_row(self._row(cf_record_id=txt["id"], retire=True), [txt])["action"],
                         dns_plan.CONFLICT)


@tagged("post_install", "-at_install")
class TestDnsRecordModel(HamsTransactionCase):
    """Tests [@ANCHOR: COMM_test_dns_record_constraints]"""

    def setUp(self):
        super().setUp()
        self.Record = self.env["cloudflare.dns.record"]
        self.Record.search([]).unlink()

    def _make(self, **kw):
        vals = {"name": "stun.hams.com", "type": "A", "content": "66.135.10.56", "proxied": False}
        vals.update(kw)
        return self.Record.create(vals)

    def test_01_ns_is_a_type_and_is_never_proxied(self):
        with self.assertRaises(ValidationError):
            self._make(name="callbook.hams.com", type="NS", content="ns1.hams.com", proxied=True)
        ok = self._make(name="callbook.hams.com", type="NS", content="ns1.hams.com", proxied=False)
        self.assertEqual(ok.type, "NS")
        with self.assertRaises(ValidationError):
            self._make(name="hams.com", type="TXT", content="v=spf1 -all", proxied=True)

    def test_02_a_nameserver_name_must_be_dns_only_whichever_row_comes_first(self):
        self._make(name="callbook.hams.com", type="NS", content="ns1.hams.com", proxied=False)
        with self.assertRaises(ValidationError):
            self._make(name="ns1.hams.com", proxied=True)
        glue = self._make(name="ns1.hams.com", proxied=False)
        with self.assertRaises(ValidationError):
            glue.proxied = True
        other = self._make(name="ns9.hams.com", proxied=True)
        with self.assertRaises(ValidationError):
            self._make(name="x.hams.com", type="NS", content="ns9.hams.com", proxied=False)
        self.assertTrue(other.proxied)

    def test_03_names_content_and_ids_are_validated(self):
        for bad in ({"name": "Stun.hams.com"}, {"name": "stun.hams.com."}, {"name": "hams"},
                    {"content": "not-an-ip"}, {"type": "AAAA", "content": "66.135.10.56"},
                    {"type": "CNAME", "content": "Host.Example.com."}, {"cf_record_id": "../../x"}):
            with self.assertRaises(ValidationError, msg=str(bad)):
                self._make(**bad)
        self._make(cf_record_id=_id(), name="ok.hams.com")
        self._make(name="*.hams.com", type="CNAME", content=TUNNEL_CNAME, proxied=True)

    def test_03b_a_row_created_without_proxied_is_dns_only_when_its_type_cannot_be_proxied(self):
        ns = self.Record.create({"name": "callbook.hams.com", "type": "NS", "content": "ns1.hams.com"})
        self.assertFalse(ns.proxied)
        a_row = self.Record.create({"name": "www.hams.com", "type": "CNAME", "content": TUNNEL_CNAME})
        self.assertTrue(a_row.proxied, "a CNAME keeps the default")

    def test_04_a_duplicate_row_is_refused(self):
        self._make()
        with self.assertRaises(ValidationError):
            self._make()

    def test_05_existing_rows_become_observe_only_in_the_migration(self):
        spec = importlib.util.spec_from_file_location("cf_dns_migration_17", MIGRATION_PATH)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        row = self._make()
        self.assertTrue(row.manage, "a new row is managed")
        self.env.cr.execute("UPDATE cloudflare_dns_record SET manage = true WHERE id = %s", [row.id])
        module.migrate(self.env.cr, "1.6")
        row.invalidate_recordset()
        self.assertFalse(row.manage)
        module.migrate(self.env.cr, False)  # a fresh install touches nothing


@tagged("post_install", "-at_install")
class TestDnsPush(HamsTransactionCase):
    """The push against an in-memory Cloudflare.
    Tests [@ANCHOR: cloudflare:COMM_dns_push_plan]
    Tests [@ANCHOR: cloudflare:COMM_dns_push_apply]
    Tests [@ANCHOR: cloudflare:COMM_dns_problems_hook]"""

    def setUp(self):
        super().setUp()
        self.Record = self.env["cloudflare.dns.record"]
        self.Record.search([]).unlink()
        fernet = self.safe_patch("odoo.addons.cloudflare.models.website.WebsiteCloudflare._get_fernet")
        fernet.return_value = Fernet(Fernet.generate_key())
        self.website = self.env["website"].create({"name": "DNS push site", "domain": "https://dnspush.example.com"})
        self.website.write({"cloudflare_api_token": "tok-dns", "cloudflare_zone_id": "z", "cloudflare_account_id": "a"})
        self.cf = FakeCloudflare(["hams.com", "perens.com"])
        for name in ("find_zone_id", "list_dns_records_named", "create_dns_record", "update_dns_record",
                     "delete_dns_record", "get_zone_details", "count_dns_records"):
            self.safe_patch(MODEL + name).side_effect = getattr(self.cf, name)

    def _row(self, name, rtype, content, proxied=False, **kw):
        return self.Record.create(dict({"name": name, "type": rtype, "content": content, "proxied": proxied,
                                        "website_id": self.website.id}, **kw))

    def _apply(self, records):
        plan = records.dns_push_plan()
        return plan, records.dns_push_apply(plan["hash"])

    def test_01_stun_hams_com_is_adopted_and_nothing_is_written_to_cloudflare(self):
        stun = self.cf.add("hams.com", "stun.hams.com", "A", "66.135.10.56", proxied=False, ttl=300)
        before = [dict(r) for r in self.cf.records]
        row = self._row("stun.hams.com", "A", "66.135.10.56")
        plan, results = self._apply(row)
        self.assertEqual([e["action"] for e in plan["entries"]], ["adopt"])
        self.assertEqual(self.cf.writes(), [], "adopting writes nothing at Cloudflare")
        self.assertEqual(self.cf.records, before)
        self.assertEqual(row.cf_record_id, stun["id"])
        self.assertEqual(results, ["adopted A stun.hams.com"])
        again = row.dns_push_plan()
        self.assertEqual([e["action"] for e in again["entries"]], ["unchanged"])

    def test_02_the_callbook_runbook_rows_are_exactly_what_is_pushed(self):
        # docs/callbook/CALLBOOK_DNS_PUBLIC_SERVING_RUNBOOK.md: two glue A rows and two NS rows, all DNS-only.
        rows = self._row("ns1.hams.com", "A", "66.135.10.56") | self._row("ns2.hams.com", "A", "66.135.10.56") \
            | self._row("callbook.hams.com", "NS", "ns1.hams.com") | self._row("callbook.hams.com", "NS", "ns2.hams.com")
        plan, results = self._apply(rows)
        zone = self.cf.zones["hams.com"]
        pushed = sorted((c[2]["type"], c[2]["name"], c[2]["content"], c[2]["proxied"], c[2]["ttl"]) for c in self.cf.writes())
        self.assertEqual(pushed, [
            ("A", "ns1.hams.com", "66.135.10.56", False, 1),
            ("A", "ns2.hams.com", "66.135.10.56", False, 1),
            ("NS", "callbook.hams.com", "ns1.hams.com", False, 1),
            ("NS", "callbook.hams.com", "ns2.hams.com", False, 1),
        ])
        self.assertTrue(all(c[1] == zone for c in self.cf.writes()))
        self.assertTrue(all(r.cf_record_id for r in rows), "created ids are stored on the rows")
        self.assertEqual({e["action"] for e in rows.dns_push_plan()["entries"]}, {"unchanged"},
                         "a second push has nothing to do")
        self.assertNotIn("DS", {c[2]["type"] for c in self.cf.writes()}, "no DS record: the callbook zone is unsigned")

    def test_03_records_odoo_has_no_row_for_are_never_touched_and_removing_a_row_deletes_nothing(self):
        # Also: archiving or unlinking a row, without the retire flag, makes no call.
        stranger = self.cf.add("hams.com", "other.hams.com", "A", "192.0.2.1")
        row = self._row("new.hams.com", "A", "192.0.2.5", proxied=True)
        self._apply(row)
        self.assertIn(stranger["id"], [r["id"] for r in self.cf.records])
        self.assertFalse(any(c[0] == "list" and c[1] == "other.hams.com" for c in self.cf.calls))
        self.cf.calls.clear()
        row.unlink()
        self.assertEqual(self.cf.calls, [], "removing an Odoo row makes no call at all")
        self.assertEqual(len(self.cf.records), 2)

    def test_04_a_conflicting_record_is_reported_and_left_alone_while_other_rows_still_push(self):
        mine = self.cf.add("hams.com", "taken.hams.com", "A", "192.0.2.1")
        clash = self._row("taken.hams.com", "A", "192.0.2.99")
        fine = self._row("free.hams.com", "A", "192.0.2.50")
        plan, _results = self._apply(clash | fine)
        actions = {e["name"]: e["action"] for e in plan["entries"]}
        self.assertEqual(actions, {"taken.hams.com": "conflict", "free.hams.com": "create"})
        self.assertEqual(mine["content"], "192.0.2.1")
        self.assertEqual(len(self.cf.writes()), 1)

    def test_05_taking_a_record_over_by_its_id_turns_the_conflict_into_a_shown_update(self):
        theirs = self.cf.add("hams.com", "taken.hams.com", "A", "192.0.2.1", ttl=120)
        row = self._row("taken.hams.com", "A", "192.0.2.99", cf_record_id=theirs["id"])
        plan, _results = self._apply(row)
        self.assertEqual(plan["entries"][0]["action"], "update")
        self.assertEqual(theirs["content"], "192.0.2.99")
        self.assertEqual(theirs["ttl"], 120, "the remote TTL is kept")

    def test_06_observe_only_rows_never_write(self):
        row = self._row("seen.hams.com", "A", "192.0.2.7", manage=False)
        plan, _results = self._apply(row)
        self.assertEqual(plan["entries"][0]["action"], "drift")
        self.assertEqual(self.cf.writes(), [])

    def test_07_apply_refuses_a_plan_that_is_out_of_date_and_writes_nothing(self):
        row = self._row("late.hams.com", "A", "192.0.2.8")
        plan = row.dns_push_plan()
        self.cf.add("hams.com", "late.hams.com", "A", "192.0.2.8")  # Cloudflare changed after the plan was shown
        with self.assertRaises(UserError):
            row.dns_push_apply(plan["hash"])
        with self.assertRaises(UserError):
            row.dns_push_apply("")
        self.assertEqual(self.cf.writes(), [])

    def test_08_a_row_without_credentials_or_a_zone_is_a_problem_entry_not_a_failure_of_the_rest(self):
        bare = self.env["website"].create({"name": "No token site", "domain": "https://notoken.example.com"})
        tenant_row = self._row("tenant.hams.com", "A", "192.0.2.20", website_id=bare.id)
        nowhere = self._row("nowhere.example.org", "A", "192.0.2.21")
        good = self._row("good.hams.com", "A", "192.0.2.22")
        plan = (tenant_row | nowhere | good).dns_push_plan()
        actions = {e["name"]: e["action"] for e in plan["entries"]}
        self.assertEqual(actions, {"tenant.hams.com": "problem", "nowhere.example.org": "problem",
                                   "good.hams.com": "create"})
        self.assertIn("no usable Cloudflare API token", [e for e in plan["entries"] if e["name"] == "tenant.hams.com"][0]["reason"])

    def test_09_rows_without_a_website_use_the_first_website_that_has_a_token(self):
        row = self.Record.create({"name": "nosite.hams.com", "type": "A", "content": "192.0.2.30", "proxied": False})
        plan = row.dns_push_plan()
        self.assertEqual(plan["entries"][0]["action"], "create")
        self.assertEqual([c[2] for c in self.cf.calls if c[0] == "find_zone"][0], "tok-dns")

    def test_10_an_unreachable_cloudflare_plans_nothing_instead_of_creating_everything(self):
        self.cf.fail_reads = True
        row = self._row("down.hams.com", "A", "192.0.2.40")
        plan = row.dns_push_plan()
        self.assertEqual(plan["entries"][0]["action"], "problem")
        self.assertEqual(row.dns_push_apply(plan["hash"]), [], "a plan of problems applies as nothing")
        self.assertEqual(self.cf.writes(), [])

    def test_11_the_problems_hook_can_veto_the_whole_push(self):
        row = self._row("veto.hams.com", "A", "192.0.2.41")
        hook = self.safe_patch("odoo.addons.cloudflare.models.dns_record.CloudflareDNSRecord._dns_problems")
        hook.return_value = ["a module said no"]
        plan = row.dns_push_plan()
        self.assertEqual(plan["problems"], ["a module said no"])
        with self.assertRaises(UserError):
            row.dns_push_apply(plan["hash"])
        self.assertEqual(self.cf.writes(), [])

    def test_12_only_an_administrator_may_plan_or_apply(self):
        row = self._row("who.hams.com", "A", "192.0.2.42")
        portal_user = new_test_user(self.env, login="dns_push_portal", groups="base.group_portal")
        with self.assertRaises(AccessError):
            row.with_user(portal_user).dns_push_plan()
        with self.assertRaises(AccessError):
            row.with_user(portal_user).dns_push_apply("x")
        self.assertEqual(self.cf.calls, [])

    def test_12b_retire_removes_the_callbook_ns_rows_and_only_those(self):
        stranger = self.cf.add("hams.com", "callbook.hams.com", "NS", "elsewhere.example.net")
        glue = self._row("ns1.hams.com", "A", "66.135.10.56")
        ns1 = self._row("callbook.hams.com", "NS", "ns1.hams.com")
        ns2 = self._row("callbook.hams.com", "NS", "ns2.hams.com")
        self._apply(glue | ns1 | ns2)
        self.cf.calls.clear()
        linked = sorted([ns1.cf_record_id, ns2.cf_record_id])
        self.assertTrue(all(linked))
        (ns1 | ns2).write({"retire": True})
        plan, results = self._apply(glue | ns1 | ns2)
        actions = sorted((e["name"], e["type"], e["action"]) for e in plan["entries"])
        self.assertEqual(actions, [("callbook.hams.com", "NS", "delete"), ("callbook.hams.com", "NS", "delete"),
                                   ("ns1.hams.com", "A", "unchanged")])
        deletes = [c for c in self.cf.calls if c[0] == "delete"]
        self.assertEqual(sorted(c[2] for c in deletes), linked, "only the two linked ids are deleted")
        remaining = {(r["name"], r["type"], r["content"]) for r in self.cf.records}
        self.assertEqual(remaining, {("callbook.hams.com", "NS", "elsewhere.example.net"),
                                     ("ns1.hams.com", "A", "66.135.10.56")},
                         "the delegation's own rows are gone; the record Odoo did not create is untouched")
        self.assertIn(stranger["id"], [r["id"] for r in self.cf.records])
        self.assertFalse((ns1 | ns2).filtered("active"), "retired rows are archived")
        self.assertEqual(sorted(results), ["deleted NS callbook.hams.com", "deleted NS callbook.hams.com"])

    def test_13_the_wizard_shows_the_plan_first_and_applies_exactly_that(self):
        row = self._row("wiz.hams.com", "A", "192.0.2.43")
        wizard = self.env["cloudflare.dns.push.wizard"].create({"record_ids": [(6, 0, row.ids)]})
        with self.assertRaises(UserError):
            wizard.action_apply()
        wizard.action_plan()
        self.assertIn("CREATE", wizard.plan_text)
        self.assertEqual(self.cf.writes(), [], "showing the plan writes nothing")
        wizard.action_apply()
        self.assertEqual(wizard.state, "done")
        self.assertEqual(len(self.cf.writes()), 1)

    def test_13b_the_wizard_offers_apply_for_a_plan_of_deletes(self):
        row = self._row("gone.hams.com", "A", "192.0.2.44")
        self._apply(row)
        row.write({"retire": True})
        wizard = self.env["cloudflare.dns.push.wizard"].create({"record_ids": [(6, 0, row.ids)]})
        wizard.action_plan()
        self.assertEqual(wizard.plan_writes, 1, "a delete is something Apply acts on, so the button must show")
        self.assertIn("DELETE", wizard.plan_text)
        wizard.action_apply()
        self.assertEqual([c[0] for c in self.cf.writes()], ["create", "delete"])
        self.assertFalse(row.active)

    def test_13c_an_ns_row_can_be_saved_from_the_form_without_touching_proxied(self):
        form = Form(self.Record)
        form.name = "callbook.hams.com"
        form.type = "NS"
        form.content = "ns1.hams.com"
        row = form.save()
        self.assertEqual((row.type, row.proxied), ("NS", False))
        txt = Form(self.Record)
        txt.name = "hams.com"
        txt.type = "TXT"
        txt.content = "v=spf1 -all"
        self.assertFalse(txt.save().proxied)

    def test_14_wizard_view_and_action_render(self):
        form = self.env["cloudflare.dns.push.wizard"].get_view(view_type="form")
        self.assertIn("plan_text", form["arch"])
        action = self.env.ref("cloudflare.action_cf_dns_push_wizard")
        self.assertEqual(action.res_model, "cloudflare.dns.push.wizard")


@tagged("post_install", "-at_install")
class TestDnsApiCalls(HamsTransactionCase):
    """The HTTP layer, with `_make_request` replaced. Tests [@ANCHOR: COMM_test_dns_api_calls]"""

    class _Response:
        def __init__(self, body, status=200):
            self._body, self.status_code = body, status

        def json(self):
            return self._body

    def setUp(self):
        super().setUp()
        self.mock = self.safe_patch("odoo.addons.cloudflare.utils.cloudflare_api._make_request")

    def test_01_find_zone_matches_the_exact_name_and_reports_a_failed_read(self):
        zid = _id()
        self.mock.return_value = self._Response({"result": [{"name": "hams.com.evil.net", "id": _id()},
                                                            {"name": "hams.com", "id": zid}]})
        self.assertEqual(cloudflare_api.find_zone_id("hams.com", "t"), (True, zid))
        self.assertEqual(self.mock.call_args.args[0], "GET")
        self.mock.return_value = self._Response({"result": []})
        self.assertEqual(cloudflare_api.find_zone_id("hams.com", "t"), (True, None))
        self.mock.return_value = None
        self.assertEqual(cloudflare_api.find_zone_id("hams.com", "t"), (False, "API Error"))

    def test_02_listing_follows_pages_and_a_failed_page_is_not_an_empty_list(self):
        zone = _id()
        pages = [self._Response({"result": [{"id": "1"}], "result_info": {"total_pages": 2}}),
                 self._Response({"result": [{"id": "2"}], "result_info": {"total_pages": 2}})]
        self.mock.side_effect = pages
        self.assertEqual(cloudflare_api.list_dns_records_named(zone, "a.hams.com", "t"), (True, [{"id": "1"}, {"id": "2"}]))
        self.mock.side_effect = [pages[0], None]
        self.assertEqual(cloudflare_api.list_dns_records_named(zone, "a.hams.com", "t"), (False, "API Error"))

    def test_03_create_posts_and_update_puts_and_ids_that_are_not_cloudflare_ids_are_refused(self):
        zone, rid = _id(), _id()
        self.mock.side_effect = None
        self.mock.return_value = self._Response({"result": {"id": rid}})
        self.assertEqual(cloudflare_api.create_dns_record(zone, {"type": "A"}, "t"), (True, rid))
        self.assertEqual(self.mock.call_args.args[0], "POST")
        self.assertEqual(cloudflare_api.update_dns_record(zone, rid, {"type": "A"}, "t"), (True, rid))
        self.assertEqual(self.mock.call_args.args[0], "PUT")
        calls = self.mock.call_count
        self.assertFalse(cloudflare_api.update_dns_record(zone, "../zones", {}, "t")[0])
        self.assertFalse(cloudflare_api.create_dns_record("x/../y", {}, "t")[0])
        self.assertFalse(cloudflare_api.list_dns_records_named("bad", "a.hams.com", "t")[0])
        self.assertEqual(self.mock.call_count, calls, "a malformed id never reaches the network layer")
        self.mock.return_value = self._Response({}, status=400)
        self.assertEqual(cloudflare_api.create_dns_record(zone, {}, "t"), (False, "API Error"))

    def test_04_delete_uses_delete_treats_404_as_gone_and_refuses_malformed_ids(self):
        zone, rid = _id(), _id()
        self.mock.return_value = self._Response({"result": {"id": rid}})
        self.assertEqual(cloudflare_api.delete_dns_record(zone, rid, "t"), (True, rid))
        self.assertEqual(self.mock.call_args.args[0], "DELETE")
        self.mock.return_value = self._Response({}, status=404)
        self.assertEqual(cloudflare_api.delete_dns_record(zone, rid, "t"), (True, rid))
        calls = self.mock.call_count
        self.assertFalse(cloudflare_api.delete_dns_record(zone, "../x", "t")[0])
        self.assertEqual(self.mock.call_count, calls)
        self.mock.return_value = None
        self.assertEqual(cloudflare_api.delete_dns_record(zone, rid, "t"), (False, "API Error"))


    def test_04_zone_details_and_record_count_are_reads_and_a_missing_total_is_a_failure(self):
        zone = _id()
        self.mock.return_value = self._Response({"result": {"plan": {"legacy_id": "pro"}, "created_on": "2014-01-25"}})
        self.assertEqual(cloudflare_api.get_zone_details(zone, "t")[1]["plan"]["legacy_id"], "pro")
        self.assertEqual(self.mock.call_args.args[0], "GET")
        self.mock.return_value = self._Response({"result": [], "result_info": {"total_count": 26}})
        self.assertEqual(cloudflare_api.count_dns_records(zone, "t"), (True, 26))
        self.assertEqual(self.mock.call_args.kwargs["params"], {"per_page": 1})
        self.mock.return_value = self._Response({"result": [], "result_info": {}})
        self.assertFalse(cloudflare_api.count_dns_records(zone, "t")[0])
        self.mock.return_value = None
        self.assertFalse(cloudflare_api.get_zone_details(zone, "t")[0])
        self.assertFalse(cloudflare_api.count_dns_records("../x", "t")[0])


@tagged("post_install", "-at_install")
class TestDnsCapacityPlan(HamsTransactionCase):
    """The record-limit arithmetic, as pure data. Tests [@ANCHOR: cloudflare:COMM_dns_capacity]"""

    def test_01_limits_by_plan(self):
        self.assertEqual(dns_plan.record_limit("free", "2024-08-31T00:00:00Z"), 1000)
        self.assertEqual(dns_plan.record_limit("free", "2024-09-01T00:00:00Z"), 200)
        self.assertEqual(dns_plan.record_limit("pro", "2014-01-25T03:59:00Z"), 3500)
        self.assertEqual(dns_plan.record_limit("business", None), 3500)
        self.assertIsNone(dns_plan.record_limit("enterprise", None))
        self.assertIsNone(dns_plan.record_limit("", None))

    def test_02_hard_stop_at_ninety_percent(self):
        self.assertIsNone(dns_plan.capacity_problem("hams.com", 3500, 3000, 100, 0))
        self.assertIsNone(dns_plan.capacity_problem("hams.com", 3500, 3149, 1, 0), "3,150 is exactly 90%: allowed")
        self.assertIn("hard stop", dns_plan.capacity_problem("hams.com", 3500, 3150, 1, 0))
        # A push that only shrinks the zone is never refused, even over the line.
        self.assertIsNone(dns_plan.capacity_problem("hams.com", 3500, 3400, 1, 5))
        self.assertIsNone(dns_plan.capacity_problem("hams.com", None, 99999, 10, 0), "no per-zone limit known")

    def test_03_a_member_delegation_is_one_label_directly_under_the_zone(self):
        ns = {"type": "NS", "name": "k6bp.hams.com"}
        self.assertTrue(dns_plan.is_member_delegation(ns, "hams.com"))
        self.assertFalse(dns_plan.is_member_delegation(dict(ns, name="x.k6bp.hams.com"), "hams.com"))
        self.assertFalse(dns_plan.is_member_delegation(dict(ns, type="A"), "hams.com"))
        self.assertFalse(dns_plan.is_member_delegation(dict(ns, name="*.hams.com"), "hams.com"))
        self.assertFalse(dns_plan.is_member_delegation(dict(ns, name="hams.com"), "hams.com"))


@tagged("post_install", "-at_install")
class TestDnsCapacityGuard(HamsTransactionCase):
    """The guard inside the push. Tests [@ANCHOR: COMM_test_dns_capacity_guard]"""

    def setUp(self):
        super().setUp()
        self.Record = self.env["cloudflare.dns.record"]
        self.Record.search([]).unlink()
        fernet = self.safe_patch("odoo.addons.cloudflare.models.website.WebsiteCloudflare._get_fernet")
        fernet.return_value = Fernet(Fernet.generate_key())
        self.website = self.env["website"].create({"name": "DNS cap site", "domain": "https://dnscap.example.com"})
        self.website.write({"cloudflare_api_token": "tok-dns", "cloudflare_zone_id": "z", "cloudflare_account_id": "a"})
        self.cf = FakeCloudflare(["hams.com"])
        for name in ("find_zone_id", "list_dns_records_named", "create_dns_record", "update_dns_record",
                     "delete_dns_record", "get_zone_details", "count_dns_records"):
            self.safe_patch(MODEL + name).side_effect = getattr(self.cf, name)

    def _row(self, name, rtype, content, proxied=False, **kw):
        return self.Record.create(dict({"name": name, "type": rtype, "content": content, "proxied": proxied,
                                        "website_id": self.website.id}, **kw))

    def _member(self, label):
        return self._row(f"{label}.hams.com", "NS", "ns1.hams.com")

    def test_20_a_member_delegation_is_planned_when_there_is_room(self):
        row = self._member("k6bp")
        plan = row.dns_push_plan()
        self.assertEqual(plan["problems"], [])
        self.assertEqual(plan["entries"][0]["action"], "create")
        self.assertEqual(plan["entries"][0]["zone_name"], "hams.com")

    def test_21_the_push_is_refused_past_ninety_percent_of_the_plan_limit(self):
        self.cf.count_override = 3150
        row = self._member("k6bp")
        plan = row.dns_push_plan()
        self.assertTrue(any("hard stop" in p for p in plan["problems"]), plan["problems"])
        with self.assertRaises(UserError):
            row.dns_push_apply(plan["hash"])
        self.assertEqual(self.cf.writes(), [])

    def test_22_a_new_free_zone_has_room_for_only_one_hundred_eighty(self):
        self.cf.plan = ("free", "2025-01-01T00:00:00Z")
        self.cf.count_override = 181
        self.assertTrue(self._member("k6bp").dns_push_plan()["problems"])
        self.cf.count_override = 179
        self.assertEqual(self._member("n0pz").dns_push_plan()["problems"], [])

    def test_23_a_push_that_cannot_read_the_count_is_refused_not_guessed(self):
        row = self._member("k6bp")
        self.safe_patch(MODEL + "count_dns_records").return_value = (False, "API Error")
        self.assertTrue(any("may not guess" in p for p in row.dns_push_plan()["problems"]))

    def test_24_an_existing_wildcard_refuses_member_delegations(self):
        self.cf.add("hams.com", "*.hams.com", "CNAME", "hams.com", proxied=False)
        plan = self._member("k6bp").dns_push_plan()
        self.assertTrue(any("wildcard record" in p for p in plan["problems"]), plan["problems"])

    def test_25_a_wildcard_created_with_delegations_in_one_push_is_refused(self):
        rows = self._member("k6bp") | self._row("*.hams.com", "CNAME", "hams.com")
        plan = rows.dns_push_plan()
        self.assertTrue(any("together" in p for p in plan["problems"]), plan["problems"])

    def test_26_a_plain_record_needs_no_wildcard_look_and_a_retire_needs_no_count(self):
        self._row("plain.hams.com", "A", "192.0.2.50").dns_push_plan()
        self.assertNotIn(("list", "*.hams.com"), self.cf.calls)
        self.cf.calls.clear()
        gone = self.cf.add("hams.com", "old.hams.com", "A", "192.0.2.51")
        row = self._row("old.hams.com", "A", "192.0.2.51", cf_record_id=gone["id"], retire=True)
        row.dns_push_plan()
        self.assertNotIn(("count", self.cf.zones["hams.com"]), self.cf.calls)


    def test_30_the_pending_action_plans_only_rows_that_wait_to_be_pushed(self):
        # Tests [@ANCHOR: COMM_test_dns_push_pending]
        # Tests [@ANCHOR: cloudflare:COMM_dns_push_pending]
        pushed = self._row("pushed.hams.com", "A", "192.0.2.60", cf_record_id="b" * 32)
        waiting = self._member("n0pz")
        gone = self._row("gone.hams.com", "A", "192.0.2.61", cf_record_id="c" * 32, retire=True)
        unmanaged = self._row("watch.hams.com", "A", "192.0.2.62", manage=False)
        action = self.Record.action_push_pending()
        wizard = self.env["cloudflare.dns.push.wizard"].browse(action["res_id"])
        self.assertEqual(wizard.state, "planned")
        self.assertEqual(set(wizard.record_ids.ids), {waiting.id, gone.id})
        self.assertNotIn(pushed, wizard.record_ids)
        self.assertNotIn(unmanaged, wizard.record_ids)
        self.assertIn("n0pz.hams.com", wizard.plan_text)
        self.assertEqual(self.cf.writes(), [], "showing the plan writes nothing")
        # With nothing waiting it says so instead of planning every record.
        self.Record.search([]).unlink()
        wizard = self.env["cloudflare.dns.push.wizard"].browse(self.Record.action_push_pending()["res_id"])
        self.assertEqual(wizard.state, "done")
        self.assertIn("No DNS row", wizard.result_text)

    def test_31_only_an_administrator_may_open_the_pending_action(self):
        # Tests [@ANCHOR: COMM_test_dns_push_pending]
        portal_user = new_test_user(self.env, login="dns_pending_portal", groups="base.group_portal")
        with self.assertRaises(AccessError):
            self.Record.with_user(portal_user).action_push_pending()
