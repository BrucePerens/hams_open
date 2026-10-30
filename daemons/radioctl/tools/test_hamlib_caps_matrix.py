#!/usr/bin/env python3
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for hamlib_caps_matrix.py, using saved program output."""
import json
import unittest
from pathlib import Path

import hamlib_caps_matrix as caps

HERE = Path(__file__).resolve().parent
DATA = HERE.parent / "data" / "hamlib-4.6.2"


def fixture(name: str) -> str:
    return (HERE / "testdata" / name).read_text()


def dump(model_id: int) -> dict:
    return caps.parse_dump(fixture("dump_caps_%d.txt" % model_id))


class RigListTests(unittest.TestCase):
    # Tests [@ANCHOR: radioctl_caps_parse_rig_list]
    def test_columns_with_spaces_in_names(self):
        rows = caps.parse_rig_list(fixture("rig_list.txt"))
        self.assertEqual(len(rows), 313)
        by_id = {r["model_id"]: r for r in rows}
        row = by_id[3073]
        self.assertEqual(row["mfg"], "Icom")
        self.assertEqual(row["model"], "IC-7300")
        self.assertEqual(row["macro"], "RIG_MODEL_IC7300")
        self.assertEqual(by_id[10]["mfg"], "N2ADR James Ahlstrom")


class DumpTests(unittest.TestCase):
    # Tests [@ANCHOR: radioctl_caps_parse_dump]
    def test_ic7300_identity_serial_timing(self):
        data = dump(3073)
        self.assertEqual(data["model_name"], "IC-7300")
        self.assertEqual(data["serial"]["min_baud"], 4800)
        self.assertEqual(data["serial"]["max_baud"], 115200)
        self.assertEqual(data["timing"]["timeout_ms"], 1000)
        self.assertEqual(data["timing"]["retries"], 3)
        self.assertTrue(data["async_data"])
        self.assertEqual(data["ctcss_tones"], 52)
        self.assertIn("SPECTRUM", data["get_functions"])
        self.assertEqual(
            data["limits"]["rit"], ["-9.999kHz", "+9.999kHz"]
        )

    # Tests [@ANCHOR: radioctl_caps_parse_dump]
    def test_levels_carry_ranges(self):
        data = dump(3073)
        levels = {e["name"]: e for e in data["get_levels"]}
        self.assertEqual(levels["KEYSPD"]["min"], 6.0)
        self.assertEqual(levels["KEYSPD"]["max"], 48.0)
        self.assertIn("SPECTRUM_EDGE", data["extra_levels"])
        keyer = {e["name"]: e for e in data["get_parameters"]}
        self.assertIn("values", keyer["KEYERTYPE"])

    # Tests [@ANCHOR: radioctl_caps_parse_dump]
    def test_ts2000_handshake_and_baud(self):
        data = dump(2014)
        self.assertEqual(data["serial"]["handshake"], "CTS/RTS")
        self.assertEqual(data["serial"]["min_baud"], 1200)
        self.assertEqual(data["timing"]["timeout_ms"], 200)

    # Tests [@ANCHOR: radioctl_caps_parse_dump]
    def test_network_rig_has_no_serial_section(self):
        data = dump(2036)
        self.assertIsNone(data["serial"])
        self.assertEqual(data["port_type"], "Network link")
        self.assertEqual(data["timing"]["post_write_delay_ms"], 20)

    # Tests [@ANCHOR: radioctl_caps_parse_ranges]
    def test_ranges_summary(self):
        ranges = dump(3073)["ranges"]
        self.assertEqual(ranges["TX"]["min_hz"], 1810000)
        self.assertEqual(ranges["TX"]["max_power_w"], 100)
        self.assertGreater(ranges["TX"]["count"], 10)
        self.assertGreater(ranges["RX"]["count"], 0)


class DerivedTests(unittest.TestCase):
    # Tests [@ANCHOR: radioctl_caps_classify_family]
    def test_family_rules(self):
        def fam(mfg, model, port="RS-232"):
            return caps.classify_family(mfg, model, port)["family"]

        self.assertEqual(fam("Icom", "IC-7300"), "icom_civ")
        self.assertEqual(fam("Xiegu", "G90"), "icom_civ")
        self.assertEqual(fam("Icom", "IC-PCR1000"), "icom_pcr_ascii")
        self.assertEqual(fam("Kenwood", "TS-2000"), "kenwood_ascii")
        self.assertEqual(fam("Kenwood", "TH-D74"), "kenwood_hh_ascii")
        self.assertEqual(fam("Yaesu", "FT-991"), "yaesu_newcat_ascii")
        self.assertEqual(
            fam("Yaesu", "FTDX-10"), "yaesu_newcat_ascii"
        )
        self.assertEqual(fam("Yaesu", "FT-817"), "yaesu_legacy")
        self.assertEqual(fam("Elecraft", "K3"), "elecraft_ascii")
        net = fam("FlexRadio", "6xxx", "Network link")
        self.assertEqual(net, "flexradio_network")
        self.assertEqual(fam("Acme", "X", "USB"), "other_usb")
        hpsdr = caps.classify_family("OpenHPSDR", "PiHPSDR", "RS-232")
        self.assertEqual(hpsdr["family"], "other_serial")
        net = caps.classify_family("OpenHPSDR", "X", "Network link")
        self.assertEqual(net["family"], "hpsdr_network")

    # Tests [@ANCHOR: radioctl_caps_vfo_model]
    def test_vfo_model(self):
        self.assertEqual(caps.vfo_model(["VFOA", "VFOB"]), "a_b")
        self.assertEqual(caps.vfo_model(["Sub", "Main"]), "main_sub")
        self.assertEqual(caps.vfo_model(["VFOA"]), "single")
        self.assertEqual(caps.vfo_model([]), "none")

    # Tests [@ANCHOR: radioctl_caps_quirk_hints]
    def test_quirk_hints_from_dump_fields(self):
        flex = caps.quirk_hints(dump(2036))
        self.assertIn("many_retries", flex)
        self.assertIn("post_write_delay", flex)
        self.assertIn("transceive_flag_E", flex)
        ic = caps.quirk_hints(dump(3073))
        self.assertIn("async_data_capable", ic)
        self.assertIn("long_timeout", ic)
        yaesu = caps.quirk_hints(dump(1035))
        self.assertIn("two_stop_bits", yaesu)

    # Tests [@ANCHOR: radioctl_caps_build_entry]
    def test_entry_without_dump_is_kept_with_error(self):
        row = {"model_id": 2, "mfg": "Hamlib", "model": "NET rigctl"}
        entry = caps.build_entry(row, None, "dump-caps exit 1")
        self.assertIsNone(entry["dump"])
        self.assertEqual(entry["dump_error"], "dump-caps exit 1")
        full = caps.build_entry(
            {"model_id": 3073, "mfg": "Icom", "model": "IC-7300"},
            dump(3073),
            "",
        )
        self.assertTrue(full["derived"]["has_scope"])
        self.assertEqual(full["derived"]["vfo_model"], "a_b")


class CommittedOutputTests(unittest.TestCase):
    """The committed matrix and document have the expected shape."""

    def matrix(self) -> dict:
        return json.loads(
            (DATA / "capability_matrix.json").read_text()
        )

    # Tests [@ANCHOR: radioctl_caps_build_matrix]
    def test_matrix_counts_and_provenance(self):
        matrix = self.matrix()
        self.assertEqual(
            matrix["schema_version"], caps.SCHEMA_VERSION
        )
        self.assertEqual(len(matrix["models"]), 313)
        failed = [m for m in matrix["models"] if not m["dump"]]
        self.assertEqual(
            [m["rig_list"]["model_id"] for m in failed], [2]
        )
        self.assertIn(
            "Hamlib 4.6.2", matrix["provenance"]["rigctl_version"]
        )
        self.assertIn(
            "no Hamlib source", matrix["provenance"]["source"]
        )

    # Tests [@ANCHOR: radioctl_caps_render_markdown]
    def test_markdown_is_rendered_from_the_matrix(self):
        text = caps.render_markdown(self.matrix()) + "\n"
        committed = (DATA / "HAMLIB_DIFFERENCES.md").read_text()
        self.assertEqual(text, committed)
        self.assertIn("No Hamlib source was read", text)

    # Tests [@ANCHOR: radioctl_caps_main]
    def test_json_rendering_is_stable(self):
        text = (DATA / "capability_matrix.json").read_text()
        self.assertEqual(caps.render_json(json.loads(text)), text)


if __name__ == "__main__":
    unittest.main()
