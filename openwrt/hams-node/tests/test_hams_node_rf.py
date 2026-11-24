# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for hams-node-rf with stand-ins for uci, ip, iw and logger: an interface marked rf is enough (zone, rf_live, station ID reminder,
WireGuard refused), mesh-point interfaces are found by type, AREDN neighbours are listed, and nothing stays when RF goes away."""

import os
import pathlib
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SBIN = HERE.parent / "files" / "usr" / "sbin"
UCI = """#!/bin/sh
f="$FAKE/uci"
case $1 in
commit) exit 0 ;;
set) k=${2%%=*}; v=${2#*=}; grep -v "^$k=" "$f" > "$f.n"; echo "$k=$v" >> "$f.n"; mv "$f.n" "$f"; exit 0 ;;
add_list) k=${2%%=*}; v=${2#*=}; echo "$k=$(grep "^$k=" "$f" | cut -d= -f2- | sed 's/$/ /')$v" > "$f.a"; grep -v "^$k=" "$f" >> "$f.a"; mv "$f.a" "$f"; exit 0 ;;
-q) [ "$2" = get ] && { grep "^$3=" "$f" | head -n 1 | cut -d= -f2-; exit 0; } ;;
esac
exit 0
"""
IP = """#!/bin/sh
case "$*" in
"-o link show dev "*) cat "$FAKE/link_$5" 2>/dev/null ;;
"-6 neigh show dev "*) cat "$FAKE/neigh_$5" 2>/dev/null ;;
esac
exit 0
"""
UP = "3: %s: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500\n"
DOWN = "3: %s: <BROADCAST,MULTICAST> mtu 1500\n"


class RfTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        (self.root / "bin").mkdir()
        for name, text in (("uci", UCI), ("ip", IP), ("iw", "#!/bin/sh\ncat \"$FAKE/iw\" 2>/dev/null\n"),
                           ("logger", "#!/bin/sh\necho \"$*\" >> \"$FAKE/log\"\n")):
            (self.root / "bin" / name).write_text(text)
            (self.root / "bin" / name).chmod(0o755)
        for n in ("hams-node-fw", "hams-node-wg"):
            (self.root / "bin" / n).symlink_to(SBIN / n)
        (self.root / "uci").write_text("hams_node_fw.main.enabled=1\nhams_node_bird.main.rf_live=0\n")
        (self.root / "owner").write_text("W1AW\n")
        self.env = dict(os.environ, PATH="%s:%s" % (self.root / "bin", os.environ["PATH"]), FAKE=str(self.root),
                        HAMS_NODE_RF_STATE=str(self.root / "state"), HAMS_NODE_OWNER=str(self.root / "owner"),
                        HAMS_NODE_INITD=str(self.root / "initd"))

    def sh(self, *args):
        return subprocess.run(["sh", *map(str, args)], env=self.env, capture_output=True, text=True)

    def uci(self):
        return (self.root / "uci").read_text()

    def test_marking_an_interface_rf_is_enough(self):
        (self.root / "uci").write_text(self.uci() + "hams_node_fw.main.rf_iface=rf0\n")
        (self.root / "link_rf0").write_text(UP % "rf0")
        result = self.sh(SBIN / "hams-node-rf", "scan")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("hams_node_bird.main.rf_live=1", self.uci())
        reminder = (self.root / "state" / "station_id_reminder").read_text()
        self.assertIn("W1AW", reminder)
        self.assertIn("no encryption", reminder)
        self.assertIn("Part 97", (self.root / "log").read_text())
        wg = self.sh(SBIN / "hams-node-wg", "apply", "rf0", "internet", self.root / "peers")
        self.assertEqual(wg.returncode, 1)
        self.assertIn("refusing WireGuard", wg.stderr)

    def test_mesh_point_by_type_joins_the_rf_zone_and_neighbours_are_listed(self):
        (self.root / "iw").write_text("phy#0\n\tInterface wlan0\n\t\tifindex 4\n\t\ttype mesh point\n\tInterface wlan1\n\t\ttype managed\n")
        (self.root / "link_wlan0").write_text(UP % "wlan0")
        (self.root / "neigh_wlan0").write_text("fe80::1 lladdr aa:bb:cc:00:00:01 REACHABLE\nfe80::2 lladdr aa:bb:cc:00:00:02 STALE\n"
                                               "fe80::3 lladdr aa:bb:cc:00:00:03 FAILED\n2001:db8::9 lladdr aa:bb:cc:00:00:09 REACHABLE\n")
        self.assertEqual(self.sh(SBIN / "hams-node-rf", "scan").returncode, 0)
        self.assertIn("hams_node_fw.main.rf_iface=wlan0", self.uci())
        self.assertNotIn("wlan1", self.uci())
        self.assertEqual(self.sh(SBIN / "hams-node-rf", "list").stdout.splitlines(), ["wlan0 fe80::1", "wlan0 fe80::2"])

    def test_no_rf_means_no_flag_reminder_or_neighbours(self):
        (self.root / "uci").write_text(self.uci() + "hams_node_fw.main.rf_iface=rf0\n")
        (self.root / "link_rf0").write_text(UP % "rf0")
        (self.root / "neigh_rf0").write_text("fe80::1 lladdr aa:bb:cc:00:00:01 REACHABLE\n")
        self.sh(SBIN / "hams-node-rf", "scan")
        (self.root / "link_rf0").write_text(DOWN % "rf0")
        self.sh(SBIN / "hams-node-rf", "scan")
        self.assertIn("hams_node_bird.main.rf_live=0", self.uci())
        self.assertFalse((self.root / "state" / "station_id_reminder").exists())
        self.assertEqual(self.sh(SBIN / "hams-node-rf", "list").stdout, "")


if __name__ == "__main__":
    unittest.main()
