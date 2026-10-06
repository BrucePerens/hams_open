# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""Runs the static rule in ai_reader_rule.py: fails when a new code path hands a raw helpdesk ticket, message
or incident body to an AI reader (docs/security/TICKET_PROMPT_INJECTION.md). The rule itself, and what it
looks at, is documented there; the cases below prove it fires and that the filtered shapes pass."""
import os

from odoo.tests.common import BaseCase, tagged

from odoo.addons.hams_helpdesk.ai_reader_rule import check_source, scan_tree

AI = "# an AI-facing file: @mcp.tool\n"  # what marks a file as one that serves or calls a model
ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))


@tagged("post_install", "-at_install", "standard")
class TestAiReaderSites(BaseCase):
    def test_01_the_repository_has_no_raw_ticket_text_reader_for_ai(self):
        # Tests [@ANCHOR: hams_helpdesk:ai_reader_sites_rule]
        found = scan_tree(ROOT)
        self.assertEqual(found, [], "raw ticket-like text handed to an AI reader; use ticket.safe_view()/mcp_safe_read() or the filter:\n" + "\n".join(found))

    def test_02_a_raw_mcp_method_on_a_ticket_model_is_caught(self):
        source = (
            "class T(models.Model):\n"
            "    _inherit = 'hams_helpdesk.ticket'\n"
            "    def mcp_dump(self):\n"
            "        return [{'text': t.description} for t in self]\n"
        )
        self.assertEqual(len(check_source(source)), 1)

    def test_03_the_same_method_through_the_filter_passes(self):
        source = (
            "class T(models.Model):\n"
            "    _inherit = 'pager.incident'\n"
            "    def mcp_dump(self):\n"
            "        return [ut.scan_text(t.description).text for t in self]\n"
        )
        self.assertEqual(check_source(source), [])

    def test_04_a_raw_daemon_read_is_caught_even_through_constants(self):
        source = AI + (
            "FIELDS = ['stage']\n"
            "TEXT = FIELDS + ['description']\n"
            "def run(client):\n"
            "    return client.execute('hams_helpdesk.ticket', 'search_read', [], TEXT)\n"
        )
        self.assertEqual(len(check_source(source)), 1)

    def test_05_a_daemon_read_of_non_text_fields_passes(self):
        source = AI + (
            "FIELDS = ['stage'] + ['priority']\n"
            "def run(client):\n"
            "    return client.execute('hams_helpdesk.ticket', 'search_read', [], FIELDS)\n"
        )
        self.assertEqual(check_source(source), [])

    def test_06_a_read_with_no_field_list_means_every_field_and_is_caught(self):
        source = AI + "def run(client):\n    return client.execute('pager.incident', 'read', [1])\n"
        self.assertEqual(len(check_source(source)), 1)

    def test_07_a_read_inside_a_function_that_uses_the_safe_view_passes(self):
        source = AI + (
            "def run(client):\n"
            "    rows = client.execute('hams_helpdesk.ticket', 'read', [1], ['name'])\n"
            "    return client.execute('hams_helpdesk.ticket', 'mcp_safe_read', [1])\n"
        )
        self.assertEqual(check_source(source), [])

    def test_09_a_plain_sync_script_that_is_not_an_ai_reader_is_not_flagged(self):
        source = "def run(client):\n    return client.execute('event.event', 'search_read', [], ['name', 'description'])\n"
        self.assertEqual(check_source(source), [])
        self.assertEqual(len(check_source(source + "CLAUDE_COMMAND = ['claude']\n")), 1)

    def test_08_the_ignore_tag_with_a_reason_silences_one_call(self):
        source = AI + (
            "def run(client):\n"
            "    return client.execute('hams_helpdesk.ticket', 'read', [1], ['name'])  # ticket-ai-ignore: staff export, no AI\n"
        )
        self.assertEqual(check_source(source), [])
