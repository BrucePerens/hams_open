# SPDX-License-Identifier: AGPL-3.0-or-later
# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
import asyncio
import json
import unittest

from odoo.exceptions import AccessError
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

# Utilize implicit namespace packages, same convention
# test_generalized_monitor.py already established -- but wrapped in a
# try/except, unlike that file: a real, pre-existing environment bug was
# found while building this (three overlapping mcp-1.28.0/1.28.1/2.0.0
# dist-info directories under /usr/local/lib/python3.13/dist-packages,
# leaving mcp.server.fastmcp.tools.tool_manager unable to import
# LifespanContextT from mcp.shared.context -- a real ImportError under the
# odoo user's own Python, confirmed NOT present under a plain user-local
# install of the same nominal version). Recorded, not fixed, in
# night_shift_todo.md -- the version choice is Bruce's own call, not
# something to force-reinstall around. A bare top-level import here would
# take pager_duty's ENTIRE test suite down with it (a single broken
# optional dependency crashing Odoo's own test collection for every other,
# unrelated test in this module) until that's resolved -- skipUnless below
# means TestPagerMcpServerModule below skips cleanly instead, and starts
# passing on its own the moment the environment is fixed, no test change
# needed.
try:
    import odoo.addons.pager_duty.daemon.pager_mcp_server as pager_mcp_server

    _MCP_IMPORT_ERROR = None
except ImportError as _e:  # audit-ignore-catch-all
    pager_mcp_server = None
    _MCP_IMPORT_ERROR = _e


@tagged("post_install", "-at_install")
class TestPagerMcpTriageModelMethods(HamsTransactionCase):
    """PAGER_DUTY_MCP_AI_TRIAGE.md's real build order slice 1:
    mcp_list_incidents/mcp_get_incident_detail/mcp_add_note on pager.incident,
    and the narrowly-scoped group_pager_mcp_triage_service account's real
    ACL boundary -- read on pager.incident, nothing else, same pattern
    test_pager_security.py's own TestPagerSecurity already establishes for
    the other personas."""

    def setUp(self):
        super().setUp()
        self.admin = self.env.ref("base.user_admin")
        self.mcp_svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "pager_duty.user_pager_mcp_triage_service"
        )
        self.incident = (
            self.env["pager.incident"]
            .with_user(self.admin)
            .create(
                {
                    "source": "mcp_triage_test",
                    "severity": "high",
                    "description": "Test incident for MCP triage tools",
                }
            )
        )

    def test_01_mcp_list_incidents_filters_by_status_and_severity(self):
        # Tests [@ANCHOR: pager_mcp_triage_tools]
        other = (
            self.env["pager.incident"]
            .with_user(self.admin)
            .create({"source": "other_source", "severity": "low", "description": "d"})
        )
        results = self.env["pager.incident"].with_user(self.mcp_svc_uid).mcp_list_incidents(
            severity="high"
        )
        ids = [r["id"] for r in results]
        self.assertIn(self.incident.id, ids)
        self.assertNotIn(other.id, ids)

    def test_02_mcp_get_incident_detail_includes_chatter(self):
        # Tests [@ANCHOR: pager_mcp_triage_tools]
        self.incident.with_user(self.mcp_svc_uid).mcp_add_note("first note")
        detail = self.incident.with_user(self.mcp_svc_uid).mcp_get_incident_detail()
        self.assertEqual(detail["id"], self.incident.id)
        self.assertEqual(detail["source"], "mcp_triage_test")
        bodies = " ".join(m["body"] or "" for m in detail["messages"])
        self.assertIn("AI Triage", bodies)
        self.assertIn("first note", bodies)

    def test_03_mcp_add_note_tags_the_message_as_ai_authored(self):
        # Tests [@ANCHOR: pager_mcp_triage_tools]
        # Sorted by id, not date: mail.message.date is second-resolution
        # (same class of gotcha as pager.incident.last_occurred elsewhere
        # in this module -- see test_incident.py's own test_10), so the
        # creation-time system message ("Pager Duty Incident created") and
        # this note posted moments later in the same test can tie on date,
        # making sorted("date") unstable about which one is actually last.
        # id is strictly increasing on insert and never ties.
        self.incident.with_user(self.mcp_svc_uid).mcp_add_note("suspect a stuck worker")
        last_message = self.incident.message_ids.sorted(key=lambda m: m.id)[-1]
        self.assertIn("🤖 AI Triage:", last_message.body)
        self.assertIn("suspect a stuck worker", last_message.body)

    def test_04_mcp_triage_service_account_can_read_but_never_write_or_create_or_unlink(self):
        # Tests [@ANCHOR: pager_mcp_triage_tools]
        # The real security boundary this whole design depends on: the MCP
        # server's own credential must be able to do exactly what its three
        # tools need (read, and call the two elevate-internally methods)
        # and nothing else -- same "violently rejected by the ORM" standard
        # test_pager_security.py's own TestPagerSecurity already holds
        # every other persona to.
        svc_incident = self.incident.with_user(self.mcp_svc_uid)
        # Allowed: read.
        svc_incident.read(["name", "source"])
        # Allowed: the two methods that internally elevate.
        svc_incident.mcp_add_note("allowed note")
        svc_incident.mcp_get_incident_detail()

        with self.assertRaises(AccessError):
            svc_incident.write({"status": "acknowledged"})
            self.env.flush_all()

        with self.assertRaises(AccessError):
            self.env["pager.incident"].with_user(self.mcp_svc_uid).create(
                {"source": "x", "severity": "low", "description": "y"}
            )
            self.env.flush_all()

        with self.assertRaises(AccessError):
            svc_incident.unlink()
            self.env.flush_all()


@tagged("post_install", "-at_install")
@unittest.skipIf(
    pager_mcp_server is None,
    f"mcp package not importable in this environment (see this file's own top-of-file note): {_MCP_IMPORT_ERROR}",
)
class TestPagerMcpServerModule(HamsTransactionCase):
    """The thin RPC-adapter layer in daemon/pager_mcp_server.py -- mocks
    OdooClient.execute() (real ORM/ACL coverage lives in
    TestPagerMcpTriageModelMethods above), confirming each tool calls the
    right model method with the right arguments, and that
    set_incident_status is genuinely not exposed as a tool."""

    def test_05_exactly_the_three_non_destructive_tools_are_registered(self):
        # Tests [@ANCHOR: pager_mcp_triage_tools]
        # PAGER_DUTY_MCP_AI_TRIAGE.md's own slice 1a: set_incident_status
        # must not be built here -- a real, deliberate absence, not an
        # oversight.
        tools = asyncio.run(pager_mcp_server.mcp.list_tools())
        tool_names = sorted(t.name for t in tools)
        self.assertEqual(
            tool_names, ["add_incident_note", "get_incident", "list_incidents"]
        )

    def test_06_list_incidents_calls_the_model_method_with_filters(self):
        # Tests [@ANCHOR: pager_mcp_triage_tools]
        mock_execute = self.safe_patch(
            "odoo.addons.pager_duty.daemon.pager_mcp_server.OdooClient.execute",
            return_value=[{"id": 1}],
        )
        self.safe_patch(
            "odoo.addons.pager_duty.daemon.pager_mcp_server.os.environ",
            {"PAGER_MCP_API_KEY": "fake-key-for-test"},
        )
        result = json.loads(pager_mcp_server.list_incidents(status="open", severity="high", limit=10))
        self.assertEqual(result, [{"id": 1}])
        mock_execute.assert_called_once_with(
            "pager.incident", "mcp_list_incidents", status="open", severity="high", limit=10
        )

    def test_07_get_incident_calls_the_model_method_with_ids(self):
        # Tests [@ANCHOR: pager_mcp_triage_tools]
        mock_execute = self.safe_patch(
            "odoo.addons.pager_duty.daemon.pager_mcp_server.OdooClient.execute",
            return_value={"id": 42},
        )
        self.safe_patch(
            "odoo.addons.pager_duty.daemon.pager_mcp_server.os.environ",
            {"PAGER_MCP_API_KEY": "fake-key-for-test"},
        )
        result = json.loads(pager_mcp_server.get_incident(42))
        self.assertEqual(result, {"id": 42})
        mock_execute.assert_called_once_with(
            "pager.incident", "mcp_get_incident_detail", ids=[42]
        )

    def test_08_add_incident_note_calls_the_model_method_with_ids_and_text(self):
        # Tests [@ANCHOR: pager_mcp_triage_tools]
        mock_execute = self.safe_patch(
            "odoo.addons.pager_duty.daemon.pager_mcp_server.OdooClient.execute",
            return_value=True,
        )
        self.safe_patch(
            "odoo.addons.pager_duty.daemon.pager_mcp_server.os.environ",
            {"PAGER_MCP_API_KEY": "fake-key-for-test"},
        )
        result = json.loads(pager_mcp_server.add_incident_note(42, "checked the logs"))
        self.assertEqual(result, {"ok": True, "incident_id": 42})
        mock_execute.assert_called_once_with(
            "pager.incident", "mcp_add_note", ids=[42], text="checked the logs"
        )

    def test_09_missing_api_key_fails_loudly_not_silently(self):
        # Tests [@ANCHOR: pager_mcp_triage_tools]
        self.safe_patch(
            "odoo.addons.pager_duty.daemon.pager_mcp_server.os.environ", {}
        )
        with self.assertRaises(RuntimeError):
            pager_mcp_server._get_client()
