#!/usr/bin/env python3
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for rig_support_matrix.py, including the staleness check of docs/RIG_SUPPORT.md."""
import json
import unittest
from pathlib import Path

import rig_support_matrix as sup

HERE = Path(__file__).resolve().parent
MODELS = json.loads((sup.HAMLIB_DIR / "capability_matrix.json").read_text())["models"]


def model(model_id, mfg="X", name="Y", family="icom_civ"):
    return {"rig_list": {"model_id": model_id, "mfg": mfg, "model": name}, "derived": {"family": family}}


class SupportMatrixTests(unittest.TestCase):
    # Tests [@ANCHOR: radioctl_support_generate]
    # Tests [@ANCHOR: radioctl_support_render]
    # Tests [@ANCHOR: radioctl_support_main]
    def test_committed_file_is_not_stale(self):
        # Fails when a description, verified.toml or the Hamlib data changed without regenerating.
        self.assertEqual(sup.generate(), sup.OUT_PATH.read_text())
        self.assertEqual(sup.main(["--check"]), 0)

    # Tests [@ANCHOR: radioctl_support_load_descriptions]
    # Tests [@ANCHOR: radioctl_support_check_consistency]
    def test_every_description_and_report_is_consistent(self):
        descriptions = sup.load_descriptions()
        self.assertGreaterEqual(len(descriptions), 27)
        self.assertEqual(sup.check_consistency(descriptions, sup.load_reports(), MODELS), [])

    # Tests [@ANCHOR: radioctl_support_load_reports]
    def test_no_report_exists_before_the_first_bench_day(self):
        reports = sup.load_reports()
        hardware = {k for k, d in sup.load_descriptions().items() if d["hardware"]}
        self.assertEqual({r["rig"] for r in reports}, hardware)

    # Tests [@ANCHOR: radioctl_support_check_consistency]
    def test_disagreements_are_reported(self):
        descriptions: dict[str, dict] = {"a": {"hardware": [{"state": "hardware-verified"}], "highest": "hardware-verified"},
                        "b": {"hardware": [], "highest": "unverified"}}
        reports = [{"rig": "b", "radio": "r", "firmware": "1", "date": "2026-11-01", "reporter": "x", "issue": "u"},
                   {"rig": "ghost", "radio": "r"}]
        text = "\n".join(sup.check_consistency(descriptions, reports, [model(1)]))
        self.assertIn("a: add it to HAMLIB_IDS", text)
        self.assertIn("b has no hardware-verified fact", text)
        self.assertIn("a: hardware-verified facts but no [[report]]", text)
        self.assertIn("'ghost' has no description", text)
        self.assertIn("lacks firmware", text)

    # Tests [@ANCHOR: radioctl_support_classify]
    # Tests [@ANCHOR: radioctl_support_status]
    def test_every_model_gets_a_tier_and_the_family_counts_match_the_plan(self):
        tiers: dict[str, set[str]] = {}
        for m in MODELS:
            family, tier, engine = sup.classify(m)
            self.assertIn(tier, "ABCD")
            tiers.setdefault(family, set()).add(tier)
            self.assertEqual(engine is not None, tier in "AB" and family != "kenwood_hh_ascii")
        self.assertEqual(sum(1 for m in MODELS if sup.classify(m)[0] == "icom_civ"), 79)
        self.assertEqual(tiers["kenwood_hh_ascii"], {"B"})
        self.assertEqual(sup.classify(model(1, "Ten-Tec", "Orion", "other_serial"))[1], "C")
        self.assertEqual(sup.classify(model(2, "SoftRock", "x", "other_usb"))[1], "D")
        self.assertEqual(sup.classify({"rig_list": {"model_id": 3, "mfg": "H", "model": "N"}, "derived": None})[0], "unclassified")
        self.assertEqual(sup.status_of("A", "civ", False), "engine only")
        self.assertEqual(sup.status_of("C", None, True), "described")
        self.assertEqual(sup.status_of("C", None, False, True), "not applicable")
        self.assertEqual(sup.status_of("C", None, True, True), "described")

    # Tests [@ANCHOR: radioctl_support_build_rows]
    def test_every_not_applicable_model_exists_and_says_why(self):
        ids = {m["rig_list"]["model_id"] for m in MODELS}
        rows = {r["id"]: r for r in sup.build_rows(MODELS, sup.load_descriptions(), sup.load_reports())}
        self.assertTrue(sup.NOT_APPLICABLE)
        for model_id, reason in sup.NOT_APPLICABLE.items():
            self.assertIn(model_id, ids)
            self.assertGreater(len(reason), 40)
            self.assertEqual(rows[model_id]["status"], "not applicable")
        self.assertEqual({rows[i]["mfg"] for i in sup.NOT_APPLICABLE}, {"Winradio"})

    # Tests [@ANCHOR: radioctl_support_build_rows]
    def test_rows_cover_every_model_once_and_ic7300_is_described(self):
        rows = sup.build_rows(MODELS, sup.load_descriptions(), sup.load_reports())
        self.assertEqual(len(rows), 313)
        self.assertEqual(len({r["id"] for r in rows}), 313)
        row = next(r for r in rows if r["id"] == 3073)
        self.assertEqual((row["status"], row["descriptions"], row["level"]), ("described", ["icom-ic-7300"], "unverified"))

    # Tests [@ANCHOR: radioctl_support_build_rows]
    def test_reporter_and_level_come_from_the_report_and_facts(self):
        descriptions = {"icom-ic-7300": {"name": "n", "family": "icom-civ", "highest": "hardware-verified", "hardware": [{}]}}
        reports = [{"rig": "icom-ic-7300", "reporter": "K6BP", "date": "2026-11-07"}]
        rows = sup.build_rows([MODELS[0], next(m for m in MODELS if m["rig_list"]["model_id"] == 3073)], descriptions, reports)
        row = next(r for r in rows if r["id"] == 3073)
        self.assertEqual((row["level"], row["who"]), ("hardware-verified", "K6BP, 2026-11-07"))


if __name__ == "__main__":
    unittest.main()
