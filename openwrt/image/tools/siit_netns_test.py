#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-or-later
"""Stateless SIIT round trip through real Jool, in network namespaces on a virtual node (stock OpenWrt x86-64 image in QEMU, no root on the host).
Usage: siit_netns_test.py IMAGE [--timeout S]. Run through agent_procs.py (about 6 minutes). Exit 0 = all passed.

The guest is the gateway (root namespace, Jool SIIT instance made by the real `hams-node-siit render` + `apply` from a registry export in the
format of ham.ipv6.siit.domain._gateway_config()); namespace "six" is an IPv6 host whose address is also inside the mesh /96, namespace "four" is
the AREDN host 10.1.2.3. Checks: ICMP, TCP and UDP both ways with the payload intact, the packets the hosts receive carry correct checksums
(tcpdump -vv with offload off), and addresses outside the loaded /96 (another domain, 64:ff9b::/96) are not translated.
The guest needs the host's internet once, for kmod-jool-netfilter, jool-tools-netfilter, ip-full, tcpdump, socat and ethtool (the stock-AREDN
build of the same Jool is tested separately, see openwrt/hams-node/README.md)."""

import argparse
import json
import pathlib
import re
import sys
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from smoke_test import BOOT_PROMPT, Console  # noqa: E402
from lan_test import push  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from ipv6_siit import siit_map  # noqa: E402

NODE = ROOT / "openwrt" / "hams-node" / "files"
BLOCK = "2001:db8:ab01::/48"
DOMAIN = 1
OTHER_DOMAIN = 2
V4_HOST = "10.1.2.3"
V6_HOST4 = "10.1.2.100"  # the IPv6 host's own address is the /96 form of this IPv4 address, so its source translates statelessly


def export():
    """The registry export: two meshes, only the first is a domain this gateway loads as a live instance (the second must stay untranslated)."""
    return json.dumps({"block": BLOCK, "domains": [
        {"mesh": "mesha", "domain": DOMAIN, "prefix96": str(siit_map.domain_prefix(BLOCK, DOMAIN))},
    ]})


ONLINE = """
sysctl -w net.ipv6.conf.all.disable_ipv6=1 >/dev/null
ip addr flush dev br-lan
ip addr add 10.0.2.15/24 dev br-lan
ip route add default via 10.0.2.2
echo nameserver 10.0.2.3 > /etc/resolv.conf
for i in 1 2 3 4 5 6; do
    apk update >/dev/null 2>&1
    apk add kmod-jool-netfilter jool-tools-netfilter ip-full kmod-veth tcpdump socat ethtool >/tmp/apk.log 2>&1 && break
    sleep 3
done
tail -n 2 /tmp/apk.log
modprobe veth
which jool_siit tcpdump socat ethtool jsonfilter
sysctl -w net.ipv6.conf.all.disable_ipv6=0
"""

SETUP = """
set -e
/etc/init.d/firewall stop 2>/dev/null || true
nft flush ruleset 2>/dev/null || true
sysctl -w net.ipv6.conf.all.forwarding=1 net.ipv4.ip_forward=1 net.ipv4.conf.all.rp_filter=0 net.ipv4.conf.default.rp_filter=0 >/dev/null
ip netns add six
ip netns add four
ip link add six0 type veth peer name six1
ip link set six1 netns six
ip link add four0 type veth peer name four1
ip link set four1 netns four
ethtool -K six0 tx off rx off >/dev/null 2>&1 || true
ethtool -K four0 tx off rx off >/dev/null 2>&1 || true
ip netns exec six ethtool -K six1 tx off rx off >/dev/null 2>&1 || true
ip netns exec four ethtool -K four1 tx off rx off >/dev/null 2>&1 || true
ip link set six0 up
ip link set four0 up
ip -6 addr add 2001:db8:ff::1/128 dev six0 nodad
ip -6 route add P96 dev six0
ip addr add 10.1.2.1/32 dev four0
ip route add V4HOST/32 dev four0
ip netns exec six sh -c 'ip link set lo up; ip link set six1 up; sysctl -w net.ipv6.conf.all.accept_dad=0 >/dev/null; ip -6 addr add V6HOSTADDR/128 dev six1 nodad; ip -6 route add 2001:db8:ff::1 dev six1; ip -6 route add default via 2001:db8:ff::1 dev six1'
ip netns exec four sh -c 'ip link set lo up; ip link set four1 up; ip addr add V4HOST/32 dev four1; ip route add 10.1.2.1 dev four1; ip route add default via 10.1.2.1 dev four1'
echo setup-done
"""

TRAFFIC = """
tmo() { n=$1; shift; "$@" & p=$!; (sleep $n; kill $p 2>/dev/null) >/dev/null 2>&1 & wait $p; }
rm -f /tmp/*.cap /tmp/rx.* /tmp/tx.bin /tmp/tx800.bin
head -c 60000 /dev/urandom > /tmp/tx.bin
head -c 800 /tmp/tx.bin > /tmp/tx800.bin
tmo 60 ip netns exec four tcpdump -l -vv -n -i four1 > /tmp/four.cap 2>&1 &
tmo 60 ip netns exec six tcpdump -l -vv -n -i six1 > /tmp/six.cap 2>&1 &
sleep 2
ip netns exec six ping -6 -c 3 -W 2 P96PREFIXV4HOST >/dev/null 2>&1; echo icmp6to4=$?
ip netns exec four ping -c 3 -W 2 V6HOST4 >/dev/null 2>&1; echo icmp4to6=$?
tmo 10 ip netns exec four socat -u TCP4-LISTEN:9000,reuseaddr OPEN:/tmp/rx.tcp46,creat,trunc &
sleep 1
ip netns exec six socat -u OPEN:/tmp/tx.bin TCP6:[P96PREFIXV4HOST]:9000
sleep 1
cmp /tmp/tx.bin /tmp/rx.tcp46 >/dev/null 2>&1; echo tcp6to4=$?
tmo 10 ip netns exec six socat -u TCP6-LISTEN:9001,reuseaddr OPEN:/tmp/rx.tcp64,creat,trunc &
sleep 1
ip netns exec four socat -u OPEN:/tmp/tx.bin TCP4:V6HOST4:9001
sleep 1
cmp /tmp/tx.bin /tmp/rx.tcp64 >/dev/null 2>&1; echo tcp4to6=$?
tmo 6 ip netns exec four socat -u UDP4-LISTEN:9002 OPEN:/tmp/rx.udp46,creat,trunc &
sleep 1
ip netns exec six socat -u OPEN:/tmp/tx800.bin UDP6:[P96PREFIXV4HOST]:9002
sleep 1
cmp /tmp/tx800.bin /tmp/rx.udp46 >/dev/null 2>&1; echo udp6to4=$?
tmo 6 ip netns exec six socat -u UDP6-LISTEN:9003 OPEN:/tmp/rx.udp64,creat,trunc &
sleep 1
ip netns exec four socat -u OPEN:/tmp/tx800.bin UDP4:V6HOST4:9003
sleep 1
cmp /tmp/tx800.bin /tmp/rx.udp64 >/dev/null 2>&1; echo udp4to6=$?
sleep 2
echo traffic-done
"""

NEGATIVE = """
ip netns exec six ping -6 -c 2 -W 2 OTHERPREFIXV4HOST >/dev/null 2>&1; echo other_domain=$?
ip netns exec six ping -6 -c 2 -W 2 64:ff9b::V4HOST >/dev/null 2>&1; echo well_known=$?
"""


def sh(con, text, timeout):
    con.send("cat > /tmp/s.sh <<'HAMS_EOF'\n%s\nHAMS_EOF\n" % text.strip().replace("\t", "    "))
    return con.run("sh /tmp/s.sh", timeout)


def out(res):
    return re.sub(r"\x1b\[[0-9;]*m", "", res[0] if res else "TIMEOUT").strip()


def fill(text):
    prefix = str(siit_map.domain_prefix(BLOCK, DOMAIN).network_address)
    return (text.replace("P96PREFIXV4HOST", str(siit_map.embed(BLOCK, DOMAIN, V4_HOST)))
            .replace("OTHERPREFIXV4HOST", str(siit_map.embed(BLOCK, OTHER_DOMAIN, V4_HOST)))
            .replace("V6HOSTADDR", str(siit_map.embed(BLOCK, DOMAIN, V6_HOST4)))
            .replace("P96", "%s/96" % prefix).replace("V6HOST4", V6_HOST4).replace("V4HOST", V4_HOST))


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
        check("jool, ip netns, veth, tcpdump, socat and ethtool are available", "/usr/bin/jool_siit" in out(res) and "/usr/bin/socat" in out(res), out(res))
        push(con, NODE / "usr" / "sbin" / "hams-node-siit", "/usr/sbin/hams-node-siit")
        con.send("cat > /tmp/export.json <<'HAMS_EOF'\n%s\nHAMS_EOF\n" % export())
        res = sh(con, "modprobe jool_siit; hams-node-siit render /tmp/export.json /tmp/siit && hams-node-siit apply /tmp/siit; echo apply=$?\njool_siit -i mesha global display | grep pool6", 60)
        check("hams-node-siit loads the instance of the registry export", "apply=0" in out(res) and str(siit_map.domain_prefix(BLOCK, DOMAIN)) in out(res), out(res))
        res = sh(con, fill(SETUP), 60)
        check("namespaces, gateway and hosts set up", "setup-done" in out(res), out(res))
        res = sh(con, fill(TRAFFIC), 120)
        text = out(res)
        for name, label in (("icmp6to4", "ICMP echo IPv6 host to AREDN host"), ("icmp4to6", "ICMP echo AREDN host to IPv6 host"),
                            ("tcp6to4", "TCP 60 kB IPv6 to AREDN, bytes identical"), ("tcp4to6", "TCP 60 kB AREDN to IPv6, bytes identical"),
                            ("udp6to4", "UDP IPv6 to AREDN, bytes identical"), ("udp4to6", "UDP AREDN to IPv6, bytes identical")):
            check(label, "%s=0" % name in text, text)
        res = sh(con, "for f in four six; do echo $f bad=$(grep -c -i 'incorrect' /tmp/$f.cap) good=$(grep -c 'cksum .* (correct)' /tmp/$f.cap); done\ngrep -i incorrect /tmp/four.cap /tmp/six.cap | head -3", 30)
        text = out(res)
        print(text)
        counts = {m[0]: (int(m[1]), int(m[2])) for m in re.findall(r"^(four|six) bad=(\d+) good=(\d+)", text, re.M)}
        check("every packet the AREDN host receives has a correct checksum", counts.get("four", (1, 0))[0] == 0 and counts.get("four", (1, 0))[1] > 20, text)
        check("every packet the IPv6 host receives has a correct checksum", counts.get("six", (1, 0))[0] == 0 and counts.get("six", (1, 0))[1] > 20, text)
        res = sh(con, fill(NEGATIVE), 60)
        text = out(res)
        check("an address in a domain this gateway does not serve is not translated", "other_domain=1" in text, text)
        check("64:ff9b::/96 is not translated", "well_known=1" in text, text)
    finally:
        con.close()
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
