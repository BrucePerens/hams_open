# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""
Bruce, 2026-11-11 (critical): nothing for a `<callsign>.hams.com` name may exist at Cloudflare; hams.com's own name
servers are the only ones that answer for it.

The guarantees under test, outermost first:

* every function in `utils/cloudflare_api.py` that creates or changes a DNS record, a custom hostname or the tunnel's
  ingress list asks `member_name_refusal` first, and nothing else in the two repositories writes to Cloudflare's
  `/dns_records` (a source scan, so a new code path fails here rather than in production);
* a callsign-shaped name is refused at the API layer (no request is made), when a row or a tunnel route is saved,
  and by the push plan (so a row that got in another way is never sent);
* removing a name is still possible: a delete, an archive and an adoption are not refused.

Tests [@ANCHOR: cloudflare:COMM_test_member_names_never_reach_cloudflare]
"""
import ast
import os
import uuid

from odoo.exceptions import UserError, ValidationError
from odoo.modules import get_module_path
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.cloudflare.utils import cloudflare_api, dns_plan
from cryptography.fernet import Fernet

from .test_dns_push import FakeCloudflare, MODEL

MEMBER_NAMES = ("k6bp.hams.com", "K6BP.hams.com.", "www.k6bp.hams.com", "k6bp.callbook.hams.com", "kp4djt.hams.com",
                "4x1ab.hams.com", "_acme-challenge.w1aw.hams.com")
OUR_NAMES = ("hams.com", "www.hams.com", "auth.hams.com", "turn.hams.com", "stun.hams.com", "ns1.hams.com",
             "ns2.hams.com", "hams1.hams.com", "relay.hams.com", "crawler.hams.com", "parked.hams.com",
             "callbook.hams.com", "acme.hams.com", "_acme-challenge.auth.hams.com", "*.hams.com",
             "*.relay.hams.com", "k6bp.example.org", "perens.com")


def _id():
    return uuid.uuid4().hex


@tagged("post_install", "-at_install")
class TestMemberNamesNeverReachCloudflare(HamsTransactionCase):
    """The name test, the API layer and the source scan. No database rows, no network."""

    def test_01_a_callsign_label_below_hams_com_is_found_and_our_own_names_are_not(self):
        # Tests [@ANCHOR: cloudflare:COMM_dns_plan_callsign_label]
        for name in MEMBER_NAMES:
            self.assertTrue(dns_plan.callsign_label_in(name), name)
        for name in OUR_NAMES:
            self.assertIsNone(dns_plan.callsign_label_in(name), name)
        self.assertIsNone(dns_plan.callsign_label_in(None))
        self.assertIsNone(dns_plan.callsign_label_in(""))

    def test_02_the_api_functions_send_nothing_for_a_member_name(self):
        # Tests [@ANCHOR: cloudflare:COMM_member_name_refusal]
        # Tests [@ANCHOR: cloudflare:COMM_dns_api_create_record]
        # Tests [@ANCHOR: cloudflare:COMM_dns_api_update_record]
        # Tests [@ANCHOR: cloudflare:COMM_cf_api_create_custom_hostname]
        # Tests [@ANCHOR: cloudflare:COMM_update_cfd_tunnel_configuration]
        mock = self.safe_patch("odoo.addons.cloudflare.utils.cloudflare_api._make_request")
        zone, rid = _id(), _id()
        for name in MEMBER_NAMES:
            ok, why = cloudflare_api.create_dns_record(zone, {"type": "NS", "name": name, "content": "ns1.hams.com"}, "t")
            self.assertFalse(ok)
            self.assertIn("refused", why)
            self.assertFalse(cloudflare_api.update_dns_record(zone, rid, {"type": "NS", "name": name}, "t")[0])
            self.assertFalse(cloudflare_api.create_custom_hostname(name, "t", zone)[0])
            payload = {"config": {"ingress": [{"hostname": name, "service": "http://odoo:8069"},
                                              {"service": "http_status:404"}]}}
            self.assertFalse(cloudflare_api.update_cfd_tunnel_configuration("acct", "t", "tun", payload)[0])
        mock.assert_not_called()

    def test_03_our_own_names_are_still_sent_and_a_delete_is_never_refused(self):
        # Tests [@ANCHOR: cloudflare:COMM_member_name_refusal]
        mock = self.safe_patch("odoo.addons.cloudflare.utils.cloudflare_api._make_request")

        class Response:
            status_code = 200

            @staticmethod
            def json():
                return {"result": {"id": "a" * 32}}

        mock.return_value = Response()
        zone = _id()
        self.assertTrue(cloudflare_api.create_dns_record(zone, {"type": "A", "name": "stun.hams.com"}, "t")[0])
        self.assertTrue(cloudflare_api.update_dns_record(zone, "a" * 32, {"type": "A", "name": "stun.hams.com"}, "t")[0])
        # Removing a member's name from Cloudflare is the one thing allowed: the cut-over deletes them.
        self.assertTrue(cloudflare_api.delete_dns_record(zone, "a" * 32, "t")[0])
        self.assertEqual([c.args[0] for c in mock.call_args_list], ["POST", "PUT", "DELETE"])

    def test_04_every_cloudflare_write_function_asks_for_the_refusal_first(self):
        # A new function that creates or changes a record, a hostname or the ingress list must carry the check.
        # Tests [@ANCHOR: cloudflare:COMM_member_name_refusal]
        path = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "utils", "cloudflare_api.py")
        with open(path, encoding="utf-8") as handle:
            tree = ast.parse(handle.read())
        writers = {"create_dns_record", "update_dns_record", "create_custom_hostname", "update_cfd_tunnel_configuration"}
        found = set()
        for node in ast.walk(tree):
            if not isinstance(node, ast.FunctionDef):
                continue
            text = ast.dump(node)
            names_a_name = any(word in text for word in ("dns_records", "custom_hostnames", "configurations"))
            writes = any(f"'{verb}'" in text for verb in ("POST", "PUT", "PATCH"))
            if names_a_name and writes and node.name != "member_name_refusal":
                found.add(node.name)
                self.assertIn("member_name_refusal", text, f"{node.name} writes to Cloudflare without the member-name check")
        self.assertTrue(writers <= found, f"the scan no longer finds {writers - found}: update this test")

    def test_05_nothing_else_in_the_repositories_writes_to_cloudflare_dns_records(self):
        # Tests [@ANCHOR: cloudflare:COMM_member_name_refusal]
        open_root = os.path.dirname(get_module_path("cloudflare"))
        roots = [open_root]
        com_root = os.path.join(os.path.dirname(open_root), "hams_com")
        if os.path.isdir(os.path.join(com_root, "daemons")):
            roots.append(com_root)
        shared_root = os.path.join(open_root, "hams_shared")
        if os.path.isdir(shared_root):
            roots.append(shared_root)
        # The one legacy library that still holds a Cloudflare POST: tenant CNAMEs built from the tenants' own
        # domains only, and its command line refuses every API call (ODOO_ONLY, see the next test).
        allowed = {
            os.path.join("cloudflare", "utils", "cloudflare_api.py"),
            os.path.join("tools", "tenant_cloudflare.py"),
        }
        skip_dirs = {".git", "node_modules", "tmp", "__pycache__", "tests", "archive", "docs", "claims", "hams_shared",
                     "target", "output"}
        offenders = []
        for root in roots:
            for current, dirs, files in os.walk(root):
                dirs[:] = [d for d in dirs if d not in skip_dirs]
                for filename in files:
                    if not filename.endswith((".py", ".rs", ".sh", ".js", ".ts")) or filename.startswith("test_"):
                        continue
                    path = os.path.join(current, filename)
                    rel = os.path.relpath(path, root)
                    if rel in allowed:
                        continue
                    try:
                        with open(path, encoding="utf-8") as handle:
                            if "/dns_records" in handle.read():
                                offenders.append(os.path.join(os.path.basename(root), rel))
                    except (OSError, UnicodeDecodeError):
                        continue
        self.assertEqual(offenders, [], "these files talk to Cloudflare's /dns_records outside the guarded functions")

    def test_06_the_legacy_tenant_tool_cannot_be_run_against_the_api(self):
        # The one allowed second writer is only safe while its entry point refuses every API command.
        # Tests [@ANCHOR: cloudflare:COMM_member_name_refusal]
        open_root = os.path.dirname(get_module_path("cloudflare"))
        path = os.path.join(open_root, "hams_shared", "tools", "tenant_cloudflare.py")
        self.assertTrue(os.path.exists(path), "hams_shared is expected beside the cloudflare module (a submodule of hams_open)")
        with open(path, encoding="utf-8") as handle:
            text = handle.read()
        self.assertIn("ODOO_ONLY", text)
        self.assertRegex(text, r"print\(ODOO_ONLY, file=sys\.stderr\)\s+return 2")
        # Its records are built from the tenants' own domains, never from members.
        self.assertIn("for domain in spec[\"domains\"]", text)


@tagged("post_install", "-at_install")
class TestMemberNameRows(HamsTransactionCase):
    """Rows, routes and the push plan, against an in-memory Cloudflare. Tests
    [@ANCHOR: cloudflare:COMM_test_member_names_never_reach_cloudflare]"""

    def setUp(self):
        super().setUp()
        self.Record = self.env["cloudflare.dns.record"]
        self.Record.search([]).unlink()
        fernet = self.safe_patch("odoo.addons.cloudflare.models.website.WebsiteCloudflare._get_fernet")
        fernet.return_value = Fernet(Fernet.generate_key())
        self.website = self.env["website"].create({"name": "Member name site", "domain": "https://members.example.com"})
        self.website.write({"cloudflare_api_token": "tok-dns", "cloudflare_zone_id": "z", "cloudflare_account_id": "a"})
        self.cf = FakeCloudflare(["hams.com"])
        for name in ("find_zone_id", "list_dns_records_named", "create_dns_record", "update_dns_record",
                     "delete_dns_record", "get_zone_details", "count_dns_records"):
            self.safe_patch(MODEL + name).side_effect = getattr(self.cf, name)

    def _row(self, name, rtype="NS", content="ns1.hams.com", **kw):
        return self.Record.create(dict({"name": name, "type": rtype, "content": content, "proxied": False,
                                        "website_id": self.website.id}, **kw))

    def _smuggle(self, row, name):
        """Gets a member name into a row the way a bad import, an old release or SQL could: past the constraint."""
        self.env.flush_all()
        self.env.cr.execute("UPDATE cloudflare_dns_record SET name = %s WHERE id = %s", (name, row.id))
        row.invalidate_recordset(["name"])

    def test_10_a_row_for_a_member_name_cannot_be_saved(self):
        # Tests [@ANCHOR: cloudflare:COMM_dns_record_no_member_names]
        for name in ("k6bp.hams.com", "www.k6bp.hams.com", "k6bp.callbook.hams.com"):
            with self.assertRaises(ValidationError), self.cr.savepoint():
                self._row(name)
                self.env.flush_all()
        plain = self._row("lab.hams.com")
        with self.assertRaises(ValidationError), self.cr.savepoint():
            plain.name = "k6bp.hams.com"
            self.env.flush_all()
        self.assertEqual(plain.name, "lab.hams.com")
        self.assertEqual(self._row("www.example.org", "CNAME", "hams.com", proxied=True).name, "www.example.org")

    def test_11_an_existing_member_row_can_still_be_retired_and_archived_but_not_revived(self):
        # Tests [@ANCHOR: cloudflare:COMM_dns_record_no_member_names]
        row = self._row("lab.hams.com", cf_record_id="b" * 32)
        self._smuggle(row, "k6bp.hams.com")
        row.write({"retire": True})
        row.write({"active": False, "retire": False, "cf_record_id": False})
        self.assertFalse(row.active)
        with self.assertRaises(ValidationError), self.cr.savepoint():
            row.active = True
            self.env.flush_all()

    def test_12_the_push_plan_refuses_a_member_row_that_got_in_another_way(self):
        # Tests [@ANCHOR: cloudflare:COMM_dns_member_name_problems]
        # Tests [@ANCHOR: cloudflare:COMM_dns_push_plan]
        # Tests [@ANCHOR: cloudflare:COMM_dns_push_apply]
        row = self._row("lab.hams.com")
        self._smuggle(row, "k6bp.hams.com")
        other = self._row("fine.hams.com", "A", "192.0.2.10")
        rows = row | other
        plan = rows.dns_push_plan()
        self.assertTrue(any("k6bp.hams.com names a callsign" in p for p in plan["problems"]), plan["problems"])
        with self.assertRaises(UserError):
            rows.dns_push_apply(plan["hash"])
        self.assertEqual(self.cf.writes(), [], "not even the harmless row is sent when the plan is refused")
        # The pending-push action shows the same refusal instead of a quiet plan.
        wizard = self.env["cloudflare.dns.push.wizard"].browse(self.Record.action_push_pending()["res_id"])
        self.assertIn("REFUSED", wizard.plan_text)
        self.assertEqual(self.cf.writes(), [])

    def test_13_deleting_a_member_record_at_cloudflare_is_planned_and_allowed(self):
        # The cut-over removes the 33 delegations; nothing may stop a retire.
        # Tests [@ANCHOR: cloudflare:COMM_dns_member_name_problems]
        remote = self.cf.add("hams.com", "k6bp.hams.com", "NS", "ns1.hams.com")
        row = self._row("lab.hams.com", cf_record_id=remote["id"])
        self._smuggle(row, "k6bp.hams.com")
        row.retire = True
        plan = row.dns_push_plan()
        self.assertEqual(plan["problems"], [])
        self.assertEqual([e["action"] for e in plan["entries"]], ["delete"])
        row.dns_push_apply(plan["hash"])
        self.assertEqual([c[0] for c in self.cf.writes()], ["delete"])
        self.assertFalse(row.active)

    def test_14_an_unchanged_or_adopted_member_row_writes_nothing_and_blocks_nothing(self):
        # Tests [@ANCHOR: cloudflare:COMM_dns_member_name_problems]
        remote = self.cf.add("hams.com", "k6bp.hams.com", "NS", "ns1.hams.com")
        row = self._row("lab.hams.com")
        self._smuggle(row, "k6bp.hams.com")
        plan = row.dns_push_plan()
        self.assertEqual([e["action"] for e in plan["entries"]], ["adopt"])
        self.assertEqual(plan["problems"], [])
        row.dns_push_apply(plan["hash"])
        self.assertEqual(row.cf_record_id, remote["id"])
        self.assertEqual(self.cf.writes(), [])

    def test_15_a_tunnel_route_cannot_carry_a_member_hostname(self):
        # Tests [@ANCHOR: cloudflare:COMM_tunnel_route_no_member_names]
        Route = self.env["cloudflare.tunnel.route"]
        with self.assertRaises(ValidationError), self.cr.savepoint():
            Route.create({"hostname": "k6bp.hams.com", "service_url": "http://odoo:8069"})
            self.env.flush_all()
        ok = Route.create({"hostname": "relay.hams.com", "service_url": "http://odoo:8069"})
        with self.assertRaises(ValidationError), self.cr.savepoint():
            ok.hostname = "w1aw.hams.com"
            self.env.flush_all()
