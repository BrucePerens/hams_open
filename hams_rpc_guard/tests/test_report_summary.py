# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
import importlib.util
import io
import os
import tempfile
from contextlib import redirect_stdout

from odoo.modules.module import get_module_path
from odoo.tests import BaseCase, tagged

PATH = os.path.join(get_module_path("hams_rpc_guard"), "tools", "report_summary.py")


def _tool():
    spec = importlib.util.spec_from_file_location("hams_rpc_guard_report_summary", PATH)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@tagged("post_install", "-at_install")
class TestReportSummary(BaseCase):
    # Tests [@ANCHOR: hams_rpc_guard:report_summary]

    def test_the_latest_count_of_each_worker_is_summed(self):
        lines = [
            "2026-11-12 10:00:01,100 4001 WARNING hams_prod odoo.addons.hams_rpc_guard.policy: RPC guard (report): 1 call(s) to ham.dx.spot.get_recent_spots by a portal user, reason strict-portal. The call was served: report-only mode.\n",
            "2026-11-12 10:05:01,100 4001 WARNING hams_prod odoo.addons.hams_rpc_guard.policy: RPC guard (report): 10 call(s) to ham.dx.spot.get_recent_spots by a portal user, reason strict-portal. The call was served: report-only mode.\n",
            "2026-11-12 10:05:02,100 4002 WARNING hams_prod odoo.addons.hams_rpc_guard.policy: RPC guard (report): 1 call(s) to ham.dx.spot.get_recent_spots by a portal user, reason strict-portal. The call was served: report-only mode.\n",
            "2026-11-12 10:06:00,000 4001 WARNING hams_prod odoo.addons.hams_rpc_guard.endpoint_gate: Served (report-only mode), would have refused, 100 request(s) to the json2 endpoints from a public client (reason: network); the count is per worker since it started. See hams_rpc_guard.allowed_networks and the README of hams_rpc_guard.\n",
            "2026-11-12 10:06:00,000 4001 WARNING hams_prod odoo.addons.hams_rpc_guard.endpoint_gate: Refused 10 request(s) to the legacy-rpc endpoints from a public client (reason: network); the count is per worker since it started.\n",
            "2026-11-12 10:06:01,000 4001 INFO hams_prod odoo.http: unrelated\n",
        ]
        self.assertEqual(_tool().summarise(lines), {
            ("method", "report", "ham.dx.spot.get_recent_spots", "portal", "strict-portal"): 11,
            ("endpoint", "report", "json2", "public", "network"): 100,
            ("endpoint", "enforce", "legacy-rpc", "public", "network"): 10,
        })

    def test_the_lines_the_guard_really_writes_are_understood(self):
        """The format strings of policy.record_verdict and endpoint_gate.note_request, rendered, match the patterns."""
        tool = _tool()
        policy_text = "RPC guard (%s): %s call(s) to %s.%s by a %s user, reason %s.%s" % (
            "report", 10, "res.partner", "read", "service", "client-not-listed", " The call was served: report-only mode.")
        line = "2026-11-12 10:00:01,100 4001 WARNING hams_prod odoo.addons.hams_rpc_guard.policy: " + policy_text
        self.assertEqual(tool.summarise([line]), {("method", "report", "res.partner.read", "service", "client-not-listed"): 10})

    def test_main_prints_totals_busiest_first_from_a_file(self):
        gate = "2026-11-12 10:06:00,000 4001 WARNING hams_prod odoo.addons.hams_rpc_guard.endpoint_gate: Refused %d request(s) to the legacy-rpc endpoints from a public client (reason: network); the count is per worker since it started.\n"
        policy = "2026-11-12 10:00:01,100 4002 WARNING hams_prod odoo.addons.hams_rpc_guard.policy: RPC guard (enforce): 7 call(s) to res.partner.read by a portal user, reason strict-portal.\n"
        with tempfile.NamedTemporaryFile("w", suffix=".log") as handle:
            handle.write(policy + gate % 10)
            handle.flush()
            out = io.StringIO()
            with redirect_stdout(out):
                self.assertEqual(_tool().main([handle.name]), 0)
        lines = out.getvalue().splitlines()
        self.assertEqual(len(lines), 2)
        self.assertIn("legacy-rpc", lines[0])
        self.assertTrue(lines[0].lstrip().startswith("10 "))
        self.assertIn("res.partner.read", lines[1])

    def test_main_says_so_when_there_is_nothing_to_count(self):
        with tempfile.NamedTemporaryFile("w", suffix=".log") as handle:
            handle.write("2026-11-12 10:06:01,000 4001 INFO hams_prod odoo.http: unrelated\n")
            handle.flush()
            out = io.StringIO()
            with redirect_stdout(out):
                self.assertEqual(_tool().main([handle.name]), 0)
        self.assertEqual(out.getvalue(), "No RPC guard would-be refusals or refusals in the input.\n")
