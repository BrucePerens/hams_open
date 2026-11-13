# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests of exposure.merge, exposure._pairs and exposure.load_exposure with fake cursors and real temporary files,
added to kill the mutants the model-level tests left alive (mutation run 2026-11-13)."""
import json
import os
import tempfile
from types import SimpleNamespace

from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

from odoo.addons.hams_rpc_guard import exposure


class _Cursor:
    def __init__(self, names):
        self.names = names
        self.executed = []

    def execute(self, query, *args):
        self.executed.append(query)

    def fetchall(self):
        return [(name,) for name in self.names]


@tagged("post_install", "-at_install")
class TestExposureLoad(HamsTransactionCase):
    # Tests [@ANCHOR: hams_rpc_guard:exposure_init]
    # Tests [@ANCHOR: hams_rpc_guard:exposure_merge]
    # Tests [@ANCHOR: hams_rpc_guard:pairs]
    # Tests [@ANCHOR: hams_rpc_guard:load_exposure]

    def test_a_new_exposure_is_empty(self):
        fresh = exposure.Exposure()
        self.assertEqual((fresh.internal, fresh.portal, fresh.stock, fresh.clients, fresh.sources, fresh.modules),
                         (set(), set(), set(), {}, {}, set()))

    def test_merge_warns_about_unknown_sections_only(self):
        fresh = exposure.Exposure()
        with self.assertLogs(exposure._logger, "WARNING") as logs:
            fresh.merge({"internal": {}, "portal": {}, "stock": {}, "clients": {}, "comment": "x", "intrnal": {},
                         "other": 1}, "mod_a")
        self.assertEqual([record.getMessage() for record in logs.records],
                         ["mod_a: unknown section 'intrnal' in rpc_exposure.json",
                          "mod_a: unknown section 'other' in rpc_exposure.json"])

    def test_merge_adds_to_every_section_and_accumulates(self):
        fresh = exposure.Exposure()
        fresh.merge({"internal": {"a.b": ["m1"]}, "portal": {"a.b": ["m2"]}, "stock": {"c.d": ["m3"]}}, "one")
        fresh.merge({"internal": {"a.b": ["m4"]}}, "two")
        self.assertEqual(fresh.internal, {("a.b", "m1"), ("a.b", "m4")})
        self.assertEqual(fresh.portal, {("a.b", "m2")})
        self.assertEqual(fresh.stock, {("c.d", "m3")})
        self.assertEqual(fresh.clients, {})

    def test_merge_clients_in_both_spellings_and_sources(self):
        fresh = exposure.Exposure()
        fresh.merge({"clients": {"daemon1": {"allow": {"a.b": ["m1"]}, "sources": ["x.py"]},
                                 "daemon2": {"a.b": ["m2"]}}}, "one")
        fresh.merge({"clients": {"daemon1": {"allow": {"a.b": ["m3"]}, "sources": ["y.py"]}}}, "two")
        self.assertEqual(fresh.clients, {"daemon1": {("a.b", "m1"), ("a.b", "m3")}, "daemon2": {("a.b", "m2")}})
        self.assertEqual(fresh.sources, {"daemon1": ["x.py", "y.py"]})

    def test_merge_a_dict_entry_without_allow_is_read_as_the_pairs_mapping_itself(self):
        fresh = exposure.Exposure()
        fresh.merge({"clients": {"d": {"a.b": ["m1"]}}}, "one")
        self.assertEqual(fresh.clients, {"d": {("a.b", "m1")}})
        self.assertEqual(fresh.sources, {})

    def test_merge_a_dict_entry_with_allow_none_grants_nothing_but_names_the_client(self):
        fresh = exposure.Exposure()
        fresh.merge({"clients": {"d": {"allow": None, "sources": "not a list"}, "e": None, "f": ["bad"]}}, "one")
        self.assertEqual(fresh.clients, {"d": set(), "e": set(), "f": set()})
        self.assertEqual(fresh.sources, {})

    def test_merge_a_clients_value_that_is_not_a_mapping_is_ignored_with_the_origin_named(self):
        for junk in (["a"], "x", 5):
            fresh = exposure.Exposure()
            with self.assertLogs(exposure._logger, "WARNING") as logs:
                fresh.merge({"clients": junk}, "mod_a")
            self.assertEqual(logs.records[0].getMessage(), "mod_a: 'clients' in rpc_exposure.json is not a mapping of "
                                                           "login to allowed methods; ignored")
            self.assertEqual(fresh.clients, {})
        fresh = exposure.Exposure()
        fresh.merge({"clients": None}, "mod_a")  # unset is not a warning
        fresh.merge({}, "mod_a")
        self.assertEqual(fresh.clients, {})

    def test_pairs_skips_everything_malformed(self):
        self.assertEqual(exposure._pairs({"a": ["m", 5, None], "b": ("n",), "c": "str", "d": 3, "e": None}),
                         {("a", "m"), ("b", "n")})
        for junk in (None, [], "x", 5, [("a", "m")]):
            self.assertEqual(exposure._pairs(junk), set())

    def _addons(self, files):
        """A temporary directory per module with the given rpc_exposure.json text (None: no file)."""
        root = tempfile.TemporaryDirectory()
        self.addCleanup(root.cleanup)
        paths = {}
        for name, text in files.items():
            folder = os.path.join(root.name, name)
            os.mkdir(folder)
            paths[name] = folder
            if text is not None:
                with open(os.path.join(folder, "rpc_exposure.json"), "w", encoding="utf-8") as handle:
                    handle.write(text)
        def module_path(name, display_warning=True):
            self.assertIs(display_warning, False, "an uninstalled or odd module must not warn")
            return paths.get(name)
        self.safe_patch_object(exposure, "get_module_path", new=module_path)
        return paths

    def test_load_merges_each_installed_addons_file_and_caches_per_registry(self):
        self._addons({
            "mod_a": json.dumps({"internal": {"a.b": ["m1"]}, "clients": {"d": {"allow": {"a.b": ["m2"]}}}}),
            "mod_b": json.dumps({"portal": {"a.b": ["m3"]}}),
            "mod_nofile": None,
        })
        cursor = _Cursor(["mod_a", "mod_b", "mod_nofile", "mod_nopath"])
        registry = SimpleNamespace()
        loaded = exposure.load_exposure(cursor, registry)
        self.assertEqual(len(cursor.executed), 1)
        self.assertIn("state = 'installed'", cursor.executed[0])
        self.assertEqual(loaded.internal, {("a.b", "m1")})
        self.assertEqual(loaded.portal, {("a.b", "m3")})
        self.assertEqual(loaded.clients, {"d": {("a.b", "m2")}})
        self.assertEqual(loaded.modules, {"mod_a", "mod_b"}, "a module with no file or no path is not listed")
        self.assertIs(registry.__dict__[exposure.CACHE_ATTRIBUTE], loaded)
        again = exposure.load_exposure(_Cursor(["never"]), registry)
        self.assertIs(again, loaded)
        other = exposure.load_exposure(_Cursor(["mod_b"]), SimpleNamespace())
        self.assertIsNot(other, loaded, "another registry reads for itself")
        self.assertEqual(other.modules, {"mod_b"})

    def test_load_skips_an_unreadable_or_malformed_file_and_logs_it(self):
        self._addons({"bad_json": "{not json", "a_list": "[1, 2]", "good": json.dumps({"stock": {"x.y": ["z"]}}),
                      "bad_utf8": None})
        cursor = _Cursor(["bad_json", "a_list", "good"])
        with self.assertLogs(exposure._logger, "ERROR") as logs:
            loaded = exposure.load_exposure(cursor, SimpleNamespace())
        self.assertEqual(len(logs.records), 1)
        self.assertEqual(logs.records[0].getMessage(), "Cannot read rpc_exposure.json of bad_json; its declarations are ignored.")
        self.assertEqual(loaded.stock, {("x.y", "z")})
        self.assertEqual(loaded.modules, {"good"}, "only a JSON object counts as a declaration")

    def test_load_with_an_undecodable_file_is_skipped(self):
        paths = self._addons({"bad_utf8": None, "good": json.dumps({"stock": {"x.y": ["z"]}})})
        with open(os.path.join(paths["bad_utf8"], "rpc_exposure.json"), "wb") as handle:
            handle.write(b"\xff\xfe\x00bad")
        with self.assertLogs(exposure._logger, "ERROR"):
            loaded = exposure.load_exposure(_Cursor(["bad_utf8", "good"]), SimpleNamespace())
        self.assertEqual(loaded.modules, {"good"})

    def test_load_a_directory_named_like_the_file_is_not_a_declaration(self):
        paths = self._addons({"mod_dir": None})
        os.mkdir(os.path.join(paths["mod_dir"], "rpc_exposure.json"))
        loaded = exposure.load_exposure(_Cursor(["mod_dir"]), SimpleNamespace())
        self.assertEqual(loaded.modules, set())

    def test_manifest_of_reads_without_warning(self):
        # Tests [@ANCHOR: hams_rpc_guard:manifest_of]
        calls = []
        manifest = object()
        self.safe_patch_object(exposure.Manifest, "for_addon", new=lambda *a, **k: calls.append((a, k)) or manifest)
        self.assertIs(exposure.manifest_of("mod_a"), manifest)
        self.assertEqual(calls, [(("mod_a",), {"display_warning": False})])
