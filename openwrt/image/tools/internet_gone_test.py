#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-or-later
"""The "internet gone" scenarios on two virtual nodes (OpenWrt x86-64 image in QEMU, no root). Usage: internet_gone_test.py IMAGE [--timeout S].

Two guests are joined by a QEMU socket link (eth1, the simulated RF link: plain BGP, nothing encrypted); each also has a user-mode NIC (br-lan)
that gives the internet while the packages (bird2, unbound) are installed and that is then cut. Checks, with the internet cut:
  A  no internet: the nodes peer over the RF link (hams-node-bird, no route server), route to each other and ping, and node A resolves a
     .ham name from its cached zone (hams-node-zone, anycast servers unreachable).
  clock: B (no GPS), A+B (no internet, no GPS) and B with a spoofed GPS feed (a year ahead) leave a sane clock (hams-node-time).
Run through agent_procs.py (about 10 minutes, needs the host's internet for the package install). Exit 0 = all passed."""

import argparse
import os
import pathlib
import re
import sys
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from smoke_test import BOOT_PROMPT, Console  # noqa: E402
from lan_test import push  # noqa: E402

SBIN = pathlib.Path(__file__).resolve().parents[2] / "hams-node" / "files" / "usr" / "sbin"
PORT = 20000 + os.getpid() % 20000
NODES = {
    "A": {"asn": "4200000001", "p60": "2001:db8:ab00:10::/60", "serial": "1", "mac": "52:54:00:00:00:0a"},
    "B": {"asn": "4200000002", "p60": "2001:db8:ab00:20::/60", "serial": "2", "mac": "52:54:00:00:00:0b"},
}
ZONE = """$ORIGIN ham.
$TTL 300
@ IN SOA ns.ham. admin.ham. 1 3600 600 86400000 300
@ IN NS ns.ham.
ns IN AAAA 2001:db8:ab00::53
node IN AAAA 2001:db8:ab00:20::1
"""


# Internet phase of a guest: the user-mode NIC on br-lan (IPv4 only: apk tries IPv6 first and fails), then bird2 and unbound.
ONLINE = """
sysctl -w net.ipv6.conf.all.disable_ipv6=1 >/dev/null
ip addr flush dev br-lan
ip addr add 10.0.2.15/24 dev br-lan
ip route add default via 10.0.2.2
echo nameserver 10.0.2.3 > /etc/resolv.conf
for i in 1 2 3 4 5 6; do
    apk update >/dev/null 2>&1
    apk add bird2 bird2c unbound-daemon >/tmp/apk.log 2>&1 && break
    sleep 3
done
tail -n 3 /tmp/apk.log
which bird birdc unbound
sysctl -w net.ipv6.conf.all.disable_ipv6=0
/etc/init.d/dnsmasq stop
"""


BIRD_SETUP = """
touch /etc/config/hams_node_bird
uci set hams_node_bird.main=bird
uci set hams_node_bird.main.enabled=1
uci set hams_node_bird.main.asn={asn}
uci set hams_node_bird.main.prefix=2001:db8:ab00::/40
uci set hams_node_bird.main.prefix60={p60}
uci set hams_node_bird.main.serial={serial}
uci add_list hams_node_bird.main.neighbor='{nasn},{nll},eth1'
uci commit hams_node_bird
ip addr add {own}/128 dev lo
hams-node-bird render /etc/bird.conf
bird -p -c /etc/bird.conf && echo parsed
bird -c /etc/bird.conf -s /tmp/bird.ctl
sleep 1
ps | grep "[b]ird"
"""
ZONE_SETUP = """
touch /etc/config/hams_node_zone
uci set hams_node_zone.main=dns
uci set hams_node_zone.main.enabled=1
uci set hams_node_zone.main.port=53
uci add_list hams_node_zone.main.primary=2001:db8:ab00::53
uci commit hams_node_zone
hams-node-zone render /tmp/ub.ext
printf 'server:\\n    username: ""\\n    chroot: ""\\n    pidfile: ""\\n    do-ip6: yes\\ninclude: /tmp/ub.ext\\n' > /tmp/ub.conf
unbound -c /tmp/ub.conf
sleep 3
nslookup node.ham 127.0.0.1
"""


def sh(con, text, timeout):
    """Run a multi-line script on the guest: pushed as a file, because the serial console truncates long lines."""
    con.send("cat > /tmp/s.sh <<'HAMS_EOF'\n%s\nHAMS_EOF\n" % text.strip().replace("\t", "    "))
    return con.run("sh /tmp/s.sh", timeout)


def rf_netdev(name):
    if name == "A":
        return ["-netdev", "socket,id=rf,listen=127.0.0.1:%d" % PORT]
    return ["-netdev", "socket,id=rf,connect=127.0.0.1:%d" % PORT]


def out(res):
    return re.sub(r"\x1b\[[0-9;]*m", "", res[0] if res else "TIMEOUT").strip()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("image")
    ap.add_argument("--timeout", type=int, default=300)
    args = ap.parse_args()
    failures = []

    def check(name, ok, detail=""):
        print("%s %s %s" % ("PASS" if ok else "FAIL", name, detail.replace("\r\n", " | ")[-300:]))
        if not ok:
            failures.append(name)

    cons = {}
    try:
        for name, node in NODES.items():
            cons[name] = Console(args.image, extra=rf_netdev(name) + ["-device", "virtio-net-pci,netdev=rf,mac=%s" % node["mac"]])
            time.sleep(2)  # the listener first
        for name, con in cons.items():
            if con.wait_for(lambda b: BOOT_PROMPT in b or None, args.timeout) is None:
                print("FAIL boot of %s" % name)
                return 1
            con.send("\n")
        time.sleep(8)
        # internet phase: packages, the cached zone, the scripts
        for name, con in cons.items():
            con.send("cat > /tmp/online.sh <<'HAMS_EOF'\n%s\nHAMS_EOF\n" % ONLINE.strip())  # short lines: the serial console truncates long ones
            res = con.run("sh /tmp/online.sh", 900)
            check("%s: bird and unbound installed" % name, "/usr/sbin/bird" in out(res) and "/usr/sbin/unbound" in out(res) and "birdc" in out(res), out(res))
            for script in ("hams-node-bird", "hams-node-zone", "hams-node-time"):
                push(con, SBIN / script, "/usr/sbin/" + script)
            sh(con, "mkdir -p /etc/hams-node\ncat > /etc/hams-node/ham.zone <<'EOF'\n%sEOF" % ZONE, 20)
        # cut the internet
        for name, con in cons.items():
            res = sh(con, "ip route del default\nip addr flush dev br-lan\nuclient-fetch -T 3 -qO /dev/null http://10.0.2.2/\necho fetch=$?", 60)
            check("%s: internet is cut" % name, "fetch=" in out(res) and "fetch=0" not in out(res), out(res))
        # the RF link
        ll = {}
        for name, con in cons.items():
            res = sh(con, "ip link set eth1 up\nsleep 4\nip -6 addr show dev eth1 | awk '/inet6 fe80/{print $2}' | cut -d/ -f1", 40)
            ll[name] = out(res).split()[-1] if out(res) else ""
            check("%s: RF link-local address" % name, ll[name].startswith("fe80"), ll[name])
        setup = {}
        for name, con in cons.items():
            other = "B" if name == "A" else "A"
            n = NODES[name]
            setup[name] = sh(con, BIRD_SETUP.format(asn=n["asn"], p60=n["p60"], serial=n["serial"], nasn=NODES[other]["asn"], nll=ll[other],
                                      own=n["p60"].split("/")[0] + "1"), 60)
        # scenario A: the nodes route over RF with no route server
        time.sleep(30)
        res = sh(cons["A"], "birdc -s /tmp/bird.ctl show protocols | grep hams_rf1", 30)
        check("A: BGP session to B over RF is Established", "Established" in out(res), out(res) + " || " + out(setup["A"]))
        res = sh(cons["A"], "ip -6 route show 2001:db8:ab00:20::/60", 20)
        check("A: route to B's /60 learned over RF", "eth1" in out(res), out(res))
        res = sh(cons["A"], "ping -6 -c 3 -W 3 -I 2001:db8:ab00:10::1 2001:db8:ab00:20::1\necho rc=$?", 40)
        check("A: ping B across the RF link, internet cut", "rc=0" in out(res), out(res))
        # .ham from the cached zone
        res = sh(cons["A"], ZONE_SETUP, 60)
        check("A: .ham name resolves from the cached zone, anycast unreachable", "2001:db8:ab00:20::1" in out(res), out(res))
        # clock: sources are scripts printing an epoch; the status file says what the node decided
        for scen, srcs, want in (
            ("B (no GPS): the NTP source over the ham network is used", [("hamntp", "echo $(( $(date +%s) + 3 )) 0.1"), ("gps", "exit 1")], "source=hamntp"),
            ("A+B (no internet, no GPS): the clock is left alone", [("ntp", "exit 1"), ("gps", "exit 1")], "source=none"),
            ("B, spoofed GPS a year ahead alone: refused", [("ntp", "exit 1"), ("gps", "echo $(( $(date +%s) + 31536000 )) 1")], "refused=jump"),
            ("B, spoofed GPS against a good source: flagged", [("gps", "echo $(( $(date +%s) + 31536000 )) 1"), ("rtc", "echo $(date +%s) 2")], "disagree=gps,rtc"),
        ):
            text = "rm -f /tmp/hams-node-time.status\ntouch /etc/config/hams_node_time\nuci -q delete hams_node_time.main\n" \
                   "uci set hams_node_time.main=time\nuci set hams_node_time.main.enabled=1\nbefore=$(date +%s)\n"
            for sname, body in srcs:
                text += "printf '%%s\\n' '#!/bin/sh' '%s' > /tmp/src_%s\nchmod 755 /tmp/src_%s\nuci add_list hams_node_time.main.source=%s=/tmp/src_%s\n" % (
                    body, sname, sname, sname, sname)
            text += "uci commit hams_node_time\nhams-node-time sync\ncat /tmp/hams-node-time.status\necho moved=$(( $(date +%s) - before ))\n"
            res = sh(cons["A"], text, 60)
            moved = out(res).split("moved=")[-1].split()[0] if "moved=" in out(res) else "999999"
            check("clock, %s" % scen, want in out(res) and abs(int(moved)) < 600, out(res))
    finally:
        for con in cons.values():
            con.close()
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
