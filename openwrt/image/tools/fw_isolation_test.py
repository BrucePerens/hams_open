#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-or-later
"""Three OpenWrt x86-64 VMs in QEMU (no root) joined by socket links: ROUTER (hams-node-fw copied in), OUTSIDER on the ham side and LANHOST on the
router's LAN. Checks that the outsider cannot reach the LAN host or the router's own ssh / LuCI / rpcd (only the port the member opened), that the
LAN host can reach the outsider, and that replies work. Usage: fw_isolation_test.py IMAGE [--timeout S]. Run through agent_procs.py. Exit 0 = pass."""

import argparse
import pathlib
import sys
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from smoke_test import BOOT_PROMPT, Console, free_port  # noqa: E402
from lan_test import push  # noqa: E402

SBIN = pathlib.Path(__file__).resolve().parents[2] / "hams-node" / "files" / "usr" / "sbin"
ROUTER_SETUP = [
    "touch /etc/config/hams_node /etc/config/hams_node_fw; uci set hams_node.main=gateway; uci set hams_node.main.prefix60=2001:db8:ab40:10::/60",
    "uci set hams_node_fw.main=fw; uci add_list hams_node_fw.main.open=tcp:8080; uci commit",
    "uci delete network.wan; uci delete network.wan6; uci delete network.@device[0].ports; uci add_list network.@device[0].ports=eth2",
    "uci set network.lan.ipaddr=10.20.0.1/24; uci set network.ham=interface; uci set network.ham.proto=static; uci set network.ham.device=eth1",
    "uci set network.ham.ipaddr=10.30.0.1; uci set network.ham.netmask=255.255.255.0; uci commit network; /etc/init.d/network restart; sleep 8",
    "echo hi > /tmp/index.html; (uhttpd -f -p 0.0.0.0:8080 -h /tmp >/dev/null 2>&1 &); sleep 1",
    "hams-node-fw ensure ham; echo ensure=$?; sleep 3; uci get hams_node_fw.main.iface",
]
OUTSIDER_SETUP = [
    "/etc/init.d/firewall stop; ip addr add 10.30.0.2/24 dev eth1; ip link set eth1 up; ip route add 10.20.0.0/24 via 10.30.0.1",
    "echo hi > /tmp/index.html; (uhttpd -f -p 0.0.0.0:8000 -h /tmp >/dev/null 2>&1 &); sleep 1",
]
LANHOST_SETUP = [
    "/etc/init.d/firewall stop; ip addr add 10.20.0.2/24 dev eth1; ip link set eth1 up; ip route add default via 10.20.0.1",
    "echo hi > /tmp/index.html; (uhttpd -f -p 0.0.0.0:8000 -h /tmp >/dev/null 2>&1 &); sleep 1",
]


def nic(link, mac, mode):
    nid = "n" + mac[-5:].replace(":", "")
    return ["-netdev", "socket,id=%s,%s=127.0.0.1:%d" % (nid, mode, link), "-device", "virtio-net-pci,netdev=%s,mac=%s" % (nid, mac)]


def banner(ip):
    """What tcp 22 on ip says within 4 s (a dropped port says nothing)."""
    return "nc %s 22 </dev/null >/tmp/ssh.out 2>&1 & sleep 4; kill %%1; head -c 8 /tmp/ssh.out; echo" % ip


def fetch(url):
    return "uclient-fetch -q -T 4 -O - %s 2>/dev/null; echo rc=$?" % url


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("image")
    ap.add_argument("--timeout", type=int, default=300)
    args = ap.parse_args()
    ham, lan = free_port(), free_port()
    router = Console(args.image, extra=[*nic(ham, "52:54:00:aa:01:01", "listen"), *nic(lan, "52:54:00:aa:02:01", "listen")])
    time.sleep(4)
    outsider = Console(args.image, extra=nic(ham, "52:54:00:aa:01:02", "connect"))
    lanhost = Console(args.image, extra=nic(lan, "52:54:00:aa:02:02", "connect"))
    vms = (router, outsider, lanhost)
    failures = []

    def check(name, ok, detail=""):
        print("%s %s %s" % ("PASS" if ok else "FAIL", name, detail))
        if not ok:
            failures.append(name)

    def out(con, cmd, timeout=60):
        res = con.run(cmd, timeout)
        return res[0] if res else "TIMEOUT"

    try:
        for con in vms:
            if con.wait_for(lambda b: BOOT_PROMPT in b or None, args.timeout) is None:
                print("FAIL boot: no console prompt (exit %s)" % con.proc.poll(), con.buf[-300:])
                return 1
            con.send("\n")
        time.sleep(8)
        push(router, SBIN / "hams-node-fw", "/usr/sbin/hams-node-fw")
        for con, cmds in ((router, ROUTER_SETUP), (outsider, OUTSIDER_SETUP), (lanhost, LANHOST_SETUP)):
            for cmd in cmds:
                res = out(con, cmd, 90)
                if "ensure=" in res:
                    check("ensure ham exits 0 and records the interface", "ensure=0" in res and "ham" in res.split("ensure=0")[-1], res.strip()[-120:])

        def ok(con, url):
            res = out(con, fetch(url))
            return "hi" in res and "rc=0" in res

        check("sanity: outsider reaches the one port the member opened (tcp 8080)", ok(outsider, "http://10.30.0.1:8080/"))
        check("outsider cannot reach the router's LuCI/rpcd (tcp 80)", not ok(outsider, "http://10.30.0.1/"))
        check("outsider cannot reach the router's ubus rpcd over http", not ok(outsider, "http://10.30.0.1/ubus"))
        res = out(outsider, banner("10.30.0.1"))
        check("outsider cannot reach the router's ssh (tcp 22)", "SSH" not in res and "not found" not in res and res != "TIMEOUT", res.strip()[-60:])
        check("sanity: the LAN host reads the router's ssh banner", "SSH" in out(lanhost, banner("10.20.0.1")))
        check("outsider cannot reach a LAN host (tcp)", not ok(outsider, "http://10.20.0.2:8000/"))
        res = out(outsider, "ping -c 2 -W 2 10.20.0.2 >/dev/null 2>&1; echo ping=$?")
        check("outsider cannot ping a LAN host", "ping=1" in res, res.strip()[-40:])
        check("sanity: the LAN host reaches the router (lan zone)", ok(lanhost, "http://10.20.0.1:8080/") or ok(lanhost, "http://10.20.0.1/"))
        check("LAN host reaches the outsider and the reply comes back", ok(lanhost, "http://10.30.0.2:8000/"))
        res = out(lanhost, "ping -c 2 -W 2 10.30.0.2 >/dev/null 2>&1; echo ping=$?")
        check("LAN host can ping the outsider", "ping=0" in res, res.strip()[-40:])
        res = out(router, "hams-node-fw check; echo check=$?")
        check("no up WireGuard interface is outside a zone (check)", "check=0" in res, res.strip()[-80:])
        if failures:
            print(out(router, "ip addr; ip route; uci show network | grep -E 'ports|ipaddr|device'"))
            print(out(lanhost, "ip -br addr; ip route"))
    finally:
        for con in vms:
            con.close()
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
