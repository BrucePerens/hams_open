# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for hams-node-lan with a stateful stand-in for uci and a recording reload_config."""

import json
import os
import pathlib
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE.parent / "files" / "usr" / "sbin" / "hams-node-lan"
PREFIX = "2001:db8:ab40:10::/60"

# uci stand-in: values in $FAKE/uci.json as {"pkg.section": {"_type": t, "opt": str|[str]}}; get/set/delete/add_list/commit, -q ignored.
UCI = r'''#!/usr/bin/env python3
import json, os, sys
path = os.environ["FAKE"] + "/uci.json"
db = json.load(open(path)) if os.path.exists(path) else {}
args = [a for a in sys.argv[1:] if a != "-q"]
cmd, rest = args[0], args[1:]
def split(name):
    parts = name.split(".")
    return ".".join(parts[:2]), (parts[2] if len(parts) > 2 else None)
if cmd == "get":
    sec, opt = split(rest[0])
    val = db.get(sec, {}).get(opt)
    if val is None:
        sys.exit(1)
    print(" ".join(val) if isinstance(val, list) else val)
    sys.exit(0)
if cmd == "set":
    name, val = rest[0].split("=", 1)
    sec, opt = split(name)
    if opt is None:
        db.setdefault(sec, {})["_type"] = val
    else:
        db.setdefault(sec, {})[opt] = val
elif cmd == "delete":
    sec, opt = split(rest[0])
    if opt is None:
        db.pop(sec, None)
    else:
        db.get(sec, {}).pop(opt, None)
elif cmd == "add_list":
    name, val = rest[0].split("=", 1)
    sec, opt = split(name)
    db.setdefault(sec, {}).setdefault(opt, []).append(val)
elif cmd == "commit":
    with open(os.environ["FAKE"] + "/commits", "a") as log:
        log.write(rest[0] + "\n")
json.dump(db, open(path, "w"))
'''


class LanTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        self.fake = self.root / "fake"
        self.fake.mkdir()
        bindir = self.root / "bin"
        bindir.mkdir()
        for name, body in {"uci": UCI, "reload_config": '#!/bin/sh\necho reload >> "$FAKE/reloads"\n'}.items():
            (bindir / name).write_text(body)
            (bindir / name).chmod(0o755)
        self.env = dict(os.environ, FAKE=str(self.fake), PATH="%s:%s" % (bindir, os.environ["PATH"]))
        self.set_uci("hams_node.main", prefix60=PREFIX)

    def set_uci(self, section, **options):
        path = self.fake / "uci.json"
        db = json.loads(path.read_text()) if path.exists() else {}
        db.setdefault(section, {}).update(options)
        path.write_text(json.dumps(db))

    def db(self):
        return json.loads((self.fake / "uci.json").read_text())

    def run_script(self, *args):
        return subprocess.run(["sh", str(SCRIPT), *args], env=self.env, capture_output=True, text=True, timeout=60)

    def reloads(self):
        path = self.fake / "reloads"
        return len(path.read_text().splitlines()) if path.exists() else 0

    def test_default_lan_gets_a_64_from_the_60_and_odhcpd_serves_it(self):
        result = self.run_script("apply")
        self.assertEqual(result.returncode, 0, result.stderr)
        db = self.db()
        self.assertEqual(db["network.hamnet"]["ip6prefix"], [PREFIX])
        self.assertEqual(db["network.hamnet"]["proto"], "static")
        self.assertEqual(db["network.lan"]["ip6assign"], "64")
        self.assertEqual(db["network.lan"]["ip6hint"], "0")
        self.assertEqual(db["dhcp.lan"]["ra"], "server")
        self.assertEqual(db["dhcp.lan"]["dhcpv6"], "server")
        self.assertEqual(self.reloads(), 1)

    def test_each_listed_lan_gets_its_own_64(self):
        self.set_uci("hams_node_lan.main", interface=["lan", "iot", "guest"])
        self.assertEqual(self.run_script("apply").returncode, 0)
        db = self.db()
        self.assertEqual([db["network.%s" % i]["ip6hint"] for i in ("lan", "iot", "guest")], ["0", "1", "2"])
        self.assertTrue(all(db["network.%s" % i]["ip6assign"] == "64" for i in ("lan", "iot", "guest")))

    def test_apply_twice_changes_nothing_more(self):
        self.run_script("apply")
        first = self.db()
        self.run_script("apply")
        self.assertEqual(self.db(), first)

    def test_a_changed_60_replaces_the_old_one(self):
        self.run_script("apply")
        self.set_uci("hams_node.main", prefix60="2001:db8:ab40:20::/60")
        self.run_script("apply")
        self.assertEqual(self.db()["network.hamnet"]["ip6prefix"], ["2001:db8:ab40:20::/60"])

    def test_not_a_60_or_unsafe_text_changes_nothing(self):
        for bad in ("", "2001:db8:ab40:10::/64", "2001:db8::/60; reboot", "10.0.0.0/60", "2001:db8:ab40:10::"):
            self.set_uci("hams_node.main", prefix60=bad)
            result = self.run_script("apply")
            self.assertEqual(result.returncode, 1, bad)
            self.assertNotIn("network.hamnet", self.db(), bad)
        self.assertEqual(self.reloads(), 0)

    def test_more_than_sixteen_lans_or_a_bad_name_is_refused(self):
        self.set_uci("hams_node_lan.main", interface=["l%d" % n for n in range(17)])
        self.assertEqual(self.run_script("apply").returncode, 1)
        self.set_uci("hams_node_lan.main", interface=["lan;x"])
        self.assertEqual(self.run_script("apply").returncode, 1)
        self.assertNotIn("network.hamnet", self.db())

    def test_remove_undoes_apply(self):
        self.run_script("apply")
        self.assertEqual(self.run_script("remove").returncode, 0)
        db = self.db()
        self.assertNotIn("network.hamnet", db)
        self.assertNotIn("ip6assign", db.get("network.lan", {}))
        self.assertNotIn("ip6hint", db.get("network.lan", {}))
        self.assertNotIn("ra", db.get("dhcp.lan", {}))
        self.assertNotIn("dhcpv6", db.get("dhcp.lan", {}))

    def test_no_prefix_yet_is_a_quiet_no_op(self):
        (self.fake / "uci.json").write_text("{}")
        result = self.run_script("apply")
        self.assertEqual(result.returncode, 1)
        self.assertEqual(self.reloads(), 0)


if __name__ == "__main__":
    unittest.main()
