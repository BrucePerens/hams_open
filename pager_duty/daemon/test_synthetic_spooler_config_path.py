# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The synthetic spooler reads the same configuration file as the monitor. Plain unittest, no Odoo:
    python3 pager_duty/daemon/test_synthetic_spooler_config_path.py
"""
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import generalized_monitor  # noqa: E402
import pager_synthetic_spooler  # noqa: E402


class SyntheticSpoolerWrite(unittest.TestCase):
    def test_a_failed_write_leaves_no_temp_file_and_keeps_the_old_spool(self):
        # Tests [@ANCHOR: pager_duty:synthetic_spooler_write_spool]
        with tempfile.TemporaryDirectory() as directory:
            spool = os.path.join(directory, "spool.json")
            pager_synthetic_spooler.write_spool({"a": 1}, spool)
            with self.assertRaises(TypeError):
                pager_synthetic_spooler.write_spool({"a": object()}, spool)  # not JSON: the write fails midway, like a full disk
            self.assertEqual(os.listdir(directory), ["spool.json"])
            with open(spool, encoding="utf-8") as handle:
                self.assertEqual(handle.read(), '{"a": 1}')


class SyntheticSpoolerConfigPath(unittest.TestCase):
    def test_synthetic_spooler_reads_the_same_config_file(self):
        # Tests [@ANCHOR: pager_duty:synthetic_spooler_resolve_config_path]
        # Tests [@ANCHOR: pager_duty:test_synthetic_spooler_reads_the_same_config_file]
        with tempfile.TemporaryDirectory() as directory:
            explicit = os.path.join(directory, "explicit.json")
            with open(os.path.join(directory, "pager_config.json"), "w", encoding="utf-8") as handle:
                handle.write("{}")
            for env in ({}, {"PAGER_CONFIG_DIR": directory}, {"PAGER_CONFIG_DIR": directory + "/none"}, {"PAGER_CONFIG_PATH": explicit}):
                self.assertEqual(
                    pager_synthetic_spooler.resolve_config_path(env), generalized_monitor.resolve_config_path(env), env)
            self.assertEqual(pager_synthetic_spooler.resolve_config_path({"PAGER_CONFIG_PATH": explicit}), explicit)
            self.assertEqual(
                pager_synthetic_spooler.resolve_config_path({"PAGER_CONFIG_DIR": directory}),
                os.path.join(directory, "pager_config.json"))


if __name__ == "__main__":
    unittest.main()
