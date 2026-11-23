#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-or-later
"""Boot an OpenWrt x86-64 image (the hams test image or a stock one) in QEMU, copy hams-node-lan in over the console, apply a /60 and check
that netifd hands br-lan a /64 inside it. Usage: lan_test.py IMAGE [--timeout S]. Run through agent_procs.py. Exit 0 = pass."""

import argparse
import pathlib
import sys
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from smoke_test import BOOT_PROMPT, Console  # noqa: E402

SCRIPT = pathlib.Path(__file__).resolve().parents[2] / "hams-node" / "files" / "usr" / "sbin" / "hams-node-lan"
PREFIX = "2001:db8:ab40:10::/60"


def push(con, local, remote):
    """Copy a text file in through a quoted here-document (this image has no base64); tabs become spaces so the console does not complete."""
    text = local.read_text().replace("\t", "    ")
    assert "\nHAMS_EOF\n" not in text
    con.send("cat > %s <<'HAMS_EOF'\n%s\nHAMS_EOF\n" % (remote, text.rstrip("\n")))
    con.run("chmod 755 %s; wc -c %s" % (remote, remote), 60)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("image")
    ap.add_argument("--timeout", type=int, default=300)
    args = ap.parse_args()
    con = Console(args.image)
    failures = []

    def check(name, ok, detail=""):
        print("%s %s %s" % ("PASS" if ok else "FAIL", name, detail))
        if not ok:
            failures.append(name)

    try:
        if con.wait_for(lambda b: BOOT_PROMPT in b or None, args.timeout) is None:
            print("FAIL boot: no console prompt")
            return 1
        con.send("\n")
        time.sleep(8)
        push(con, SCRIPT, "/usr/sbin/hams-node-lan")
        res = con.run("touch /etc/config/hams_node; uci set hams_node.main=gateway; uci set hams_node.main.prefix60=%s; uci commit hams_node; hams-node-lan apply; echo rc=$?" % PREFIX, 60)
        check("apply exits 0", bool(res) and "rc=0" in res[0], (res[0] if res else "")[-200:])
        time.sleep(10)
        res = con.run("ip -6 addr show dev br-lan scope global", 30)
        out = res[0] if res else ""
        check("br-lan has a /64 inside the /60", "2001:db8:ab40:1" in out and "/64" in out, out.strip().replace("\r\n", " | ")[-300:])
        res = con.run("ubus call network.interface.lan status | grep -A3 'ipv6-prefix-assignment'; ps | grep '[o]dhcpd'", 30)
        out = res[0] if res else ""
        check("netifd assignment and odhcpd running", "2001:db8:ab40:1" in out and "odhcpd" in out, out.strip().replace("\r\n", " | ")[-300:])
        res = con.run("hams-node-lan remove; echo rc=$?; sleep 25; ip -6 addr show dev br-lan scope global | grep -c 2001:db8; ifstatus lan | grep -c 2001:db8", 90)
        check("remove: netifd no longer lists the /64", bool(res) and "rc=0" in res[0] and res[0].split("rc=0")[-1].split()[-1:] == ["0"], (res[0] if res else "")[-120:])
    finally:
        con.close()
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
