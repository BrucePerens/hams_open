#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-or-later
"""On-demand WireGuard tunnels on a virtual node (stock OpenWrt x86-64 image in QEMU, no root on the host). Usage: ondemand_test.py IMAGE [--timeout S].

One guest is the router; inside it, network namespaces "lan" (a client of the router's /60) and "far" (a member's router with a real WireGuard
interface) make real peers. `hams-node-ondemand` and its init script run unchanged (real nft ruleset, `nft monitor`, `wg`); only the directory
lookup is a stand-in `uclient-fetch`. Checks: the first packet to a member /60 creates the peer and is delivered, idle expiry removes it,
a suspended and an unknown /60 create nothing, the peer table stays at its bound (the eviction order is in the unit tests), an rf link is refused, and the memory
of the daemon with a full table is printed and held to MEM_LIMIT_KB. The guest needs the host's internet once, for wireguard-tools and ip-full.
Run through agent_procs.py (about 8 minutes). Exit 0 = all passed."""

import argparse
import pathlib
import re
import sys
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from smoke_test import BOOT_PROMPT, Console  # noqa: E402
from lan_test import push  # noqa: E402

NODE = pathlib.Path(__file__).resolve().parents[2] / "hams-node" / "files"
MEM_LIMIT_KB = 8192  # documented limit: all hams-node-ondemand processes together (daemon, nft monitor, expiry loop), 256 peers

ONLINE = """
sysctl -w net.ipv6.conf.all.disable_ipv6=1 >/dev/null
ip addr flush dev br-lan
ip addr add 10.0.2.15/24 dev br-lan
ip route add default via 10.0.2.2
echo nameserver 10.0.2.3 > /etc/resolv.conf
for i in 1 2 3 4 5 6; do
    apk update >/dev/null 2>&1
    apk add wireguard-tools kmod-wireguard ip-full kmod-veth >/tmp/apk.log 2>&1 && break
    sleep 3
done
tail -n 2 /tmp/apk.log
modprobe wireguard
modprobe veth
which wg nft jsonfilter
ip link add dummy-t type veth peer name dummy-u && echo veth=ok && ip link del dummy-t
sysctl -w net.ipv6.conf.all.disable_ipv6=0
"""

# The stand-in directory: far (2001:db8:ab00:20::/60) is the live peer, ab00:30:: is suspended, ab01:N0::/60 is any of 256 members with no live endpoint.
FETCH = """#!/bin/sh
for a in "$@"; do case $a in *query*) q=$a ;; esac; done
q=$(echo "$q" | sed 's/.*"query":"\\([^"]*\\)".*/\\1/')
g4=$(echo "$q" | cut -d: -f4)
ok() { # prefix key endpoint
    printf '{"result":{"status":"success","prefix60":"%s","public_key":"%s","endpoint":"%s"}}\\n' "$1" "$2" "$3"
}
case $q in
2001:db8:ab00:20:*) ok 2001:db8:ab00:20::/60 "$(cat /tmp/far.pub)" 10.99.0.2:51820 ;;
2001:db8:ab01:*) ok "2001:db8:ab01:$g4::/60" "$(sed -n "$((0x$g4 / 16 + 1))p" /tmp/keys)" 10.99.0.9:51820 ;;
2001:db8:ab00:30:*) echo '{"result":{"status":"error","message":"suspended"}}' ;;
*) echo '{"result":{"status":"error","message":"not found"}}' ;;
esac
"""

SETUP = """
rm -f /tmp/dir.log
mv /bin/uclient-fetch /bin/uclient-fetch.real
cp /tmp/fetch.sh /bin/uclient-fetch
/etc/init.d/firewall stop 2>/dev/null
nft flush ruleset
sysctl -w net.ipv6.conf.all.forwarding=1 >/dev/null
ip netns add lan
ip netns add far
ip link add lan0 type veth peer name lan1
ip link set lan1 netns lan
ip link add far0 type veth peer name far1
ip link set far1 netns far
ip link set lan0 up
ip addr add 10.99.0.1/24 dev far0
ip link set far0 up
ip -6 addr add 2001:db8:ab00:10::1/64 dev lan0 nodad
ip netns exec lan sh -c 'ip link set lo up; ip link set lan1 up; ip -6 addr add 2001:db8:ab00:10::2/64 dev lan1 nodad; ip -6 route add default via 2001:db8:ab00:10::1'
ip netns exec far sh -c 'ip link set lo up; ip link set far1 up; ip addr add 10.99.0.2/24 dev far1'
wg genkey > /tmp/far.key
wg pubkey < /tmp/far.key > /tmp/far.pub
wg genkey > /tmp/rt.key
wg pubkey < /tmp/rt.key > /tmp/rt.pub
ip link add wg-hams type wireguard
wg set wg-hams private-key /tmp/rt.key listen-port 51821
ip link set wg-hams up
ip -6 route add 2001:db8:ab00::/40 dev wg-hams
ip -n far link add wgfar type wireguard
ip netns exec far sh -c "wg set wgfar private-key /tmp/far.key listen-port 51820 peer $(cat /tmp/rt.pub) allowed-ips 2001:db8:ab00:10::/60"
ip netns exec far sh -c 'ip link set wgfar up; ip -6 addr add 2001:db8:ab00:20::1/128 dev wgfar; ip -6 route add 2001:db8:ab00:10::/60 dev wgfar'
touch /etc/config/hams_node_ondemand
uci set hams_node_ondemand.main=ondemand
uci set hams_node_ondemand.main.enabled=1
uci set hams_node_ondemand.main.lookup_url=https://directory.invalid/lookup
uci set hams_node_ondemand.main.ham_prefix=2001:db8:ab00::/40
uci set hams_node_ondemand.main.idle_minutes=1
uci commit hams_node_ondemand
echo setup-done
"""

KEYS = """
i=0
while [ $i -lt 256 ]; do wg genkey | wg pubkey; i=$((i + 1)); done > /tmp/keys
wc -l < /tmp/keys
"""

PEERS = "wg show wg-hams peers | wc -l"
PING = "ip netns exec lan ping -6 -c %d -W 1 %s >/dev/null 2>&1; echo ping=$?"

# the daemon's processes: every process whose command line mentions hams-node-ondemand, plus its nft monitor
MEM = """
tot=0
for p in /proc/[0-9]*; do
    c=$(tr '\\0' ' ' < $p/cmdline 2>/dev/null)
    case "$c" in *hams-node-ondemand*|*"nft monitor"*) ;; *) continue ;; esac
    r=$(awk '/VmRSS/{print $2}' $p/status 2>/dev/null)
    [ -n "$r" ] && tot=$((tot + r))
done
echo rss_kb=$tot
echo memavail_kb=$(awk '/MemAvailable/{print $2}' /proc/meminfo)
"""


def sh(con, text, timeout):
    con.send("cat > /tmp/s.sh <<'HAMS_EOF'\n%s\nHAMS_EOF\n" % text.strip().replace("\t", "    "))
    return con.run("sh /tmp/s.sh", timeout)


def out(res):
    return re.sub(r"\x1b\[[0-9;]*m", "", res[0] if res else "TIMEOUT").strip()


def number(res, name):
    found = re.search(name + r"=(\d+)", out(res))
    return int(found.group(1)) if found else -1


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

    con = Console(args.image)
    try:
        if con.wait_for(lambda b: BOOT_PROMPT in b or None, args.timeout) is None:
            print("FAIL boot")
            return 1
        con.send("\n")
        time.sleep(8)
        res = sh(con, ONLINE, 900)
        check("wireguard, ip netns and veth are available", "veth=ok" in out(res) and "/usr/bin/wg" in out(res), out(res))
        push(con, NODE / "usr" / "sbin" / "hams-node-ondemand", "/usr/sbin/hams-node-ondemand")
        push(con, NODE / "etc" / "init.d" / "hams-node-ondemand", "/etc/init.d/hams-node-ondemand")
        sh(con, "cat > /tmp/fetch.sh <<'EOF'\n%sEOF\nchmod 755 /tmp/fetch.sh" % FETCH, 20)
        res = sh(con, KEYS, 300)
        check("256 stand-in member keys made", out(res).split()[-1] == "256", out(res))
        res = sh(con, SETUP, 120)
        check("namespaces, router and far WireGuard set up", "setup-done" in out(res) and "not found" not in out(res), out(res))
        # an rf link is refused outright
        res = sh(con, "hams-node-ondemand -i wg-hams -t rf -u x -s /tmp/od-rf packet 2001:db8:ab00:20::1; echo rc=$?", 30)
        check("an rf link is refused", "rc=1" in out(res) and "refusing" in out(res), out(res))
        res = sh(con, "/etc/init.d/hams-node-ondemand start\nsleep 3\nnft list set inet hams_ondemand seen | head -3\nps w | grep -c '[h]ams-node-ondemand'", 30)
        check("init script loads the nft set and starts the daemon", "set seen" in out(res) and out(res).split()[-1] not in ("0", "1"), out(res))
        # first packet to a live member /60: made and delivered
        res = sh(con, PING % (6, "2001:db8:ab00:20::1"), 60)
        check("first packets to a member /60 are delivered through a made peer", "ping=0" in out(res), out(res))
        res = sh(con, PEERS + "\nwg show wg-hams allowed-ips | grep -c 2001:db8:ab00:20::/60", 20)
        check("the peer exists with the /60 as allowed-ips", out(res).split()[-2:] == ["1", "1"], out(res))
        # suspended and unknown /60s create nothing
        sh(con, PING % (2, "2001:db8:ab00:30::1"), 30)
        sh(con, PING % (2, "2001:db8:ab00:99::1"), 30)
        res = sh(con, PEERS + "\nwg show wg-hams allowed-ips | grep -c '2001:db8:ab00:[39]'", 20)
        check("a suspended /60 and an unknown /60 create nothing", out(res).split()[-2:] == ["1", "0"], out(res))
        # idle expiry (idle 1 minute; the expiry loop runs every minute)
        time.sleep(150)
        res = sh(con, PEERS + "\nwg show wg-hams peers | grep -c \"$(cat /tmp/far.pub)\"", 20)
        check("idle expiry removed the peer", out(res).split()[-2:] == ["0", "0"], out(res))
        res = sh(con, PING % (6, "2001:db8:ab00:20::1"), 60)
        check("traffic after expiry makes the peer again", "ping=0" in out(res), out(res))
        # full table: restart with a long idle time, make 257 peers (bound 256), measure memory
        sh(con, "uci set hams_node_ondemand.main.idle_minutes=30\nuci commit hams_node_ondemand\n/etc/init.d/hams-node-ondemand restart\nsleep 3", 30)
        before = number(sh(con, MEM, 30), "memavail_kb")
        sh(con, PING % (3, "2001:db8:ab00:20::1"), 60)
        burst = "i=0\nwhile [ $i -lt 256 ]; do ip netns exec lan ping -6 -c 1 -W 1 2001:db8:ab01:$(printf %x $((i * 16)))::1 >/dev/null 2>&1; i=$((i + 1)); done\necho burst-done"
        res = sh(con, burst, 1500)
        check("256 further first-packets handled", "burst-done" in out(res), out(res))
        time.sleep(5)
        res = sh(con, PEERS, 20)
        count = int(out(res).split()[-1]) if out(res).split()[-1].isdigit() else -1
        check("peer table is held at the bound of 256", count == 256, out(res))
        res = sh(con, MEM, 30)
        rss, after = number(res, "rss_kb"), number(res, "memavail_kb")
        print("MEMORY userspace RSS of hams-node-ondemand processes with %d peers: %d kB (limit %d kB); MemAvailable %d kB -> %d kB (%d kB incl. kernel peers)" % (
            count, rss, MEM_LIMIT_KB, before, after, before - after))
        check("memory with a full table stays under the documented limit", 0 < rss <= MEM_LIMIT_KB, out(res))
        sh(con, "/etc/init.d/hams-node-ondemand stop", 20)
    finally:
        con.close()
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
