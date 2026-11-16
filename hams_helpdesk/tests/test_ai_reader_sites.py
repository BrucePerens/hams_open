# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""Runs the static rule in ai_reader_rule.py: fails when a new code path hands a raw helpdesk ticket, message
or incident body to an AI reader (docs/security/TICKET_PROMPT_INJECTION.md). The rule itself, and what it
looks at, is documented there; the cases below prove it fires and that the filtered shapes pass."""
import os
import subprocess
import tempfile

from odoo.tests.common import BaseCase, tagged

from odoo.addons.hams_helpdesk.ai_reader_rule import _tracked_files, check_source, scan_tree

AI = "# an AI-facing file: @mcp.tool\n"  # what marks a file as one that serves or calls a model
ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))


@tagged("post_install", "-at_install", "standard")
class TestAiReaderSites(BaseCase):
    def test_01_the_repository_has_no_raw_ticket_text_reader_for_ai(self):
        # Tests [@ANCHOR: hams_helpdesk:ai_reader_sites_rule]
        # A checkout must be scanned through git, never by quietly falling back to walking the directory:
        # that fallback is what lets an untracked stray directory in.
        if os.path.exists(os.path.join(ROOT, ".git")):
            self.assertIsNotNone(_tracked_files(ROOT), "git ls-files failed in a git checkout; the scan would walk untracked files")
        found = scan_tree(ROOT)
        self.assertEqual(found, [], "raw ticket-like text handed to an AI reader; use ticket.safe_view()/mcp_safe_read() or the filter:\n" + "\n".join(found))

    def test_02_a_raw_mcp_method_on_a_ticket_model_is_caught(self):
        # Tests [@ANCHOR: hams_helpdesk:ai_reader_check_source]
        source = (
            "class T(models.Model):\n"
            "    _inherit = 'hams_helpdesk.ticket'\n"
            "    def mcp_dump(self):\n"
            "        return [{'text': t.description} for t in self]\n"
        )
        self.assertEqual(len(check_source(source)), 1)

    def test_03_the_same_method_through_the_filter_passes(self):
        # Tests [@ANCHOR: hams_helpdesk:ai_reader_markers]
        source = (
            "class T(models.Model):\n"
            "    _inherit = 'pager.incident'\n"
            "    def mcp_dump(self):\n"
            "        return [ut.scan_text(t.description).text for t in self]\n"
        )
        self.assertEqual(check_source(source), [])

    def test_04_a_raw_daemon_read_is_caught_even_through_constants(self):
        # Tests [@ANCHOR: hams_helpdesk:ai_reader_constants]
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
        # Tests [@ANCHOR: hams_helpdesk:ai_reader_ignored]
        source = AI + (
            "def run(client):\n"
            "    return client.execute('hams_helpdesk.ticket', 'read', [1], ['name'])  # ticket-ai-ignore: staff export, no AI\n"
        )
        self.assertEqual(check_source(source), [])

    def _write(self, root, rel, text):
        path = os.path.join(root, rel)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(text)

    def test_10_only_tracked_files_are_scanned_so_an_untracked_stray_directory_cannot_fail_it(self):
        # Tests [@ANCHOR: hams_helpdesk:ai_reader_sites_rule]
        # Tests [@ANCHOR: hams_helpdesk:ai_reader_tracked_files]
        # Tests [@ANCHOR: hams_helpdesk:ai_reader_candidate_files]
        # Tests [@ANCHOR: hams_helpdesk:ai_reader_py_files]
        # A stray untracked directory (a leftover nested checkout, a scratch copy) once failed the
        # repository scan with violations that are not this repository's code.
        bad = AI + "def run(client):\n    return client.execute('pager.incident', 'read', [1])\n"
        with tempfile.TemporaryDirectory() as root:
            git = ["git", "-C", root, "-c", "user.name=t", "-c", "user.email=t@example.invalid"]
            subprocess.run(git + ["init", "-q"], check=True)
            self._write(root, "tracked_ok/ok.py", "X = 1\n")
            self._write(root, "tracked_bad/bad.py", bad)
            subprocess.run(git + ["add", "."], check=True)
            subprocess.run(git + ["commit", "-q", "-m", "x"], check=True)
            self._write(root, "stray/hams_open/untracked_bad/bad.py", bad)
            found = scan_tree(root)
            self.assertEqual(len(found), 1, found)
            self.assertTrue(found[0].startswith("tracked_bad/bad.py:"), found)

    def test_11_a_directory_that_is_not_a_git_checkout_is_still_walked(self):
        # Tests [@ANCHOR: hams_helpdesk:ai_reader_sites_rule]
        bad = AI + "def run(client):\n    return client.execute('pager.incident', 'read', [1])\n"
        with tempfile.TemporaryDirectory() as root:
            self._write(root, "mod/bad.py", bad)
            found = scan_tree(root)
            self.assertEqual(len(found), 1, found)

    def test_12_a_syntax_error_source_reports_nothing_instead_of_raising(self):
        # Tests [@ANCHOR: hams_helpdesk:ai_reader_check_source]
        self.assertEqual(check_source("def broken(:\n"), [])

    def test_13_an_inherit_list_naming_a_ticket_model_is_still_a_ticket_model(self):
        # Tests [@ANCHOR: hams_helpdesk:ai_reader_check_source]
        source = (
            "class T(models.Model):\n"
            "    _inherit = ['mail.thread', 'hams_helpdesk.ticket']\n"
            "    def mcp_dump(self):\n"
            "        return [t.body for t in self]\n"
        )
        self.assertEqual(len(check_source(source)), 1)

    def test_14_an_mcp_method_on_an_unrelated_model_or_a_non_mcp_method_or_a_write_is_not_flagged(self):
        # Tests [@ANCHOR: hams_helpdesk:ai_reader_check_source]
        other = "class T(models.Model):\n    _name = 'res.partner'\n    def mcp_dump(self):\n        return [t.description for t in self]\n"
        plain = "class T(models.Model):\n    _inherit = 'hams_helpdesk.ticket'\n    def dump(self):\n        return [t.description for t in self]\n"
        store = "class T(models.Model):\n    _inherit = 'hams_helpdesk.ticket'\n    def mcp_set(self):\n        self.description = 'x'\n"
        for source in (other, plain, store):
            self.assertEqual(check_source(source), [])

    def test_15_the_ignore_tag_on_the_line_above_a_def_silences_that_method_only(self):
        # Tests [@ANCHOR: hams_helpdesk:ai_reader_ignored]
        body = "class T(models.Model):\n    _inherit = 'hams_helpdesk.ticket'\n%s    def mcp_dump(self):\n        return [t.name for t in self]\n"
        self.assertEqual(check_source(body % "    # ticket-ai-ignore: staff only\n"), [])
        self.assertEqual(len(check_source(body % "    # an ordinary comment\n")), 1)

    def test_16_a_fields_keyword_or_an_empty_field_list_is_read_as_text(self):
        # Tests [@ANCHOR: hams_helpdesk:ai_reader_check_source]
        kw = AI + "def run(client):\n    return client.execute_kw('pager.incident', 'search_read', [], fields=['description'])\n"
        empty = AI + "def run(client):\n    return client.execute('pager.incident', 'search_read', [], [])\n"
        ok = AI + "def run(client):\n    return client.execute('pager.incident', 'search_read', [], fields=['stage'])\n"
        self.assertEqual(len(check_source(kw)), 1)
        self.assertEqual(len(check_source(empty)), 1)
        self.assertEqual(check_source(ok), [])

    def test_17_other_calls_and_other_models_and_other_methods_are_not_reads(self):
        # Tests [@ANCHOR: hams_helpdesk:ai_reader_check_source]
        for call in (
            "client.fetch('pager.incident', 'read', [1])",
            "client.execute('pager.incident')",
            "client.execute('res.partner', 'read', [1])",
            "client.execute('pager.incident', 'write', [1], {'name': 'x'})",
        ):
            self.assertEqual(check_source(AI + "def run(client):\n    return %s\n" % call), [], call)

    def test_18_skipped_directories_test_files_and_non_daemon_files_are_handled(self):
        # Tests [@ANCHOR: hams_helpdesk:ai_reader_candidate_files]
        # Tests [@ANCHOR: hams_helpdesk:ai_reader_py_files]
        bad = AI + "def run(client):\n    return client.execute('pager.incident', 'read', [1])\n"
        with tempfile.TemporaryDirectory() as root:
            for rel in ("tests/bad.py", "node_modules/bad.py", ".hidden/bad.py", "mod/test_bad.py", "mod/notes.txt"):
                self._write(root, rel, bad)
            self._write(root, "mod/daemon/bad.py", bad)
            self._write(root, "mod/other/bad.py", bad)
            self.assertEqual(sorted(f.split(":")[0] for f in scan_tree(root)), ["mod/daemon/bad.py", "mod/other/bad.py"])
            self.assertEqual([f.split(":")[0] for f in scan_tree(root, daemon_only=True)], ["mod/daemon/bad.py"])
            # the same tree inside a git checkout takes the tracked-files path and skips the same directories
            git = ["git", "-C", root, "-c", "user.name=t", "-c", "user.email=t@example.invalid"]
            subprocess.run(git + ["init", "-q"], check=True)
            subprocess.run(git + ["add", "-f", "."], check=True)
            subprocess.run(git + ["commit", "-q", "-m", "x"], check=True)
            self.assertEqual(sorted(f.split(":")[0] for f in scan_tree(root)), ["mod/daemon/bad.py", "mod/other/bad.py"])
            self.assertEqual([f.split(":")[0] for f in scan_tree(root, daemon_only=True)], ["mod/daemon/bad.py"])
