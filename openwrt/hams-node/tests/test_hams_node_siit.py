# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for the hams-node-siit script: its Jool files must equal siit_map.gateway_configs."""

import json
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE.parent / "files" / "usr" / "sbin" / "hams-node-siit"
sys.path.insert(0, str(HERE.parents[2] / "ipv6_siit"))
import siit_map  # noqa: E402

BLOCK = "2001:db8:ab01::/48"


def export(domains):
    return {"block": BLOCK, "domains": domains}


def row(mesh, domain, with_prefix=True):
    prefix = str(siit_map.domain_prefix(BLOCK, domain)) if with_prefix else None
    return {"mesh": mesh, "domain": domain, "prefix96": prefix}


class RenderTests(unittest.TestCase):
    def render(self, data):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        src = pathlib.Path(tmp.name) / "export.json"
        src.write_text(json.dumps(data))
        out = pathlib.Path(tmp.name) / "siit"
        env = dict(os.environ, PATH="%s:%s" % (HERE, os.environ["PATH"]))
        result = subprocess.run(["sh", str(SCRIPT), "render", str(src), str(out)], env=env, capture_output=True, text=True)
        files = {p.stem: p.read_text() for p in out.glob("*.json")} if out.exists() else {}
        return result, files

    def test_files_equal_the_python_generator(self):
        data = export([row("sfmesh", 1), row("pdx-mesh", 7), row("m_3", 65535)])
        result, files = self.render(data)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(files, siit_map.gateway_configs(data))

    def test_unprovisioned_row_is_skipped(self):
        data = export([row("sfmesh", 1), row("later", 2, with_prefix=False)])
        result, files = self.render(data)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(sorted(files), ["sfmesh"])

    def test_unsafe_mesh_name_is_not_written(self):
        data = export([row("../evil", 1), row("ok", 2)])
        result, files = self.render(data)
        self.assertEqual(sorted(files), ["ok"])
        self.assertIn("unsafe", result.stderr)

    def test_empty_export_writes_nothing(self):
        result, files = self.render({"block": None, "domains": []})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(files, {})

    def test_missing_export_file_fails(self):
        result = subprocess.run(["sh", str(SCRIPT), "render", "/nonexistent/x.json", "/tmp/unused-hams-node"], capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)


if __name__ == "__main__":
    unittest.main()
