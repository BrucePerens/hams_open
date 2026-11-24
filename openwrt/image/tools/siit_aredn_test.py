#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-or-later
"""The SIIT round trip through Jool on the STOCK AREDN 4.26.10.0 x86-64 image (QEMU, no root on the host), three virtual machines.
Usage: siit_aredn_test.py AREDN_IMAGE HOST_IMAGE APKDIR [--timeout S]. Exit 0 = all passed. About 6 minutes; run through agent_procs.py.

The AREDN kernel has no network namespaces, so the namespaces of siit_netns_test.py are replaced by two more VMs on QEMU socket links: the gateway
(AREDN, with the Jool apks from APKDIR, built from AREDN's own tree by openwrt/aredn-jool, served over loopback HTTP; the instance is loaded by
the real hams-node-siit from a registry export), an IPv6 host and the AREDN host 10.1.2.3 (both stock OpenWrt HOST_IMAGE, with socat and tcpdump
from its feed). The checks are those of siit_netns_test.py: ICMP, TCP and UDP both ways with identical payloads, correct checksums, no
translation outside the loaded /96."""

import argparse
import functools
import http.server
import pathlib
import re
import sys
import threading
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from smoke_test import BOOT_PROMPT, Console, free_port, parse, wrap  # noqa: E402
from lan_test import push  # noqa: E402
import siit_netns_test as base  # noqa: E402

siit_map = base.siit_map


def nic(name, mac, link):
    return ["-netdev", "socket,id=%s,%s" % (name, link), "-device", "virtio-net-pci,netdev=%s,mac=%s" % (name, mac)]


HOST_ONLINE = """
sysctl -w net.ipv6.conf.all.disable_ipv6=1 >/dev/null
ip addr flush dev br-lan
ip addr add 10.0.2.15/24 dev br-lan
ip route add default via 10.0.2.2
echo nameserver 10.0.2.3 > /etc/resolv.conf
for i in 1 2 3 4 5 6; do
    apk update >/dev/null 2>&1
    apk add socat tcpdump ethtool ip-full >/tmp/apk.log 2>&1 && break
    sleep 3
done
which socat tcpdump ethtool ip
sysctl -w net.ipv6.conf.all.disable_ipv6=0
/etc/init.d/firewall stop 2>/dev/null
nft flush ruleset 2>/dev/null
ethtool -K eth1 tx off rx off tso off gso off gro off >/dev/null 2>&1
ip link set eth1 up
ip route del default
ip -6 route flush default 2>/dev/null
"""

GW_ONLINE = """
ip link set eth1 up
ip addr add 10.0.2.15/24 dev eth1
ip route add default via 10.0.2.2
mkdir -p /tmp/j && cd /tmp/j
for f in $(wget -q -O - http://10.0.2.2:PORT/ | sed -n 's/.*href="\\([^"]*[.]apk\\)".*/\\1/p'); do wget -q http://10.0.2.2:PORT/$f; done
ls | wc -l
apk add --allow-untrusted ./*.apk >/tmp/apk2.log 2>&1; echo jool-apk=$?
cd /
modprobe jool_siit
cat /proc/self/ns/net >/dev/null && ls /proc/self/ns | tr '\\n' ' '
uname -r
"""

GW_SETUP = """
/etc/init.d/firewall stop 2>/dev/null
nft flush ruleset 2>/dev/null
sysctl -w net.ipv6.conf.all.forwarding=1 net.ipv4.ip_forward=1 net.ipv4.conf.all.rp_filter=0 net.ipv4.conf.default.rp_filter=0 >/dev/null
ethtool -K eth2 tx off rx off tso off gso off gro off >/dev/null 2>&1
ethtool -K eth3 tx off rx off tso off gso off gro off >/dev/null 2>&1
ip link set eth2 up
ip link set eth3 up
ip -6 addr add 2001:db8:ff::1/128 dev eth2
ip -6 route add P96 dev eth2
ip addr add 10.1.2.1/32 dev eth3
ip route add V4HOST/32 dev eth3
echo gw-done
"""

SIX_SETUP = """
ip -6 addr add V6HOSTADDR/128 dev eth1 nodad
ip -6 route add 2001:db8:ff::1 dev eth1
ip -6 route add default via 2001:db8:ff::1 dev eth1
echo six-done
"""

FOUR_SETUP = """
ip addr add V4HOST/32 dev eth1
ip route add 10.1.2.1 dev eth1
ip route add default via 10.1.2.1 dev eth1
echo four-done
"""

TMO = 'tmo() { n=$1; shift; "$@" & p=$!; (sleep $n; kill $p 2>/dev/null) >/dev/null 2>&1 & wait $p; }'
SEND = "head -c %d /tmp/tx.bin | socat -u - %s; echo sent=$?"
CHECK = "head -c %d /tmp/tx.bin | cmp - /tmp/rx.bin >/dev/null 2>&1; echo cmp=$?"


def listen(con, address):
    """Start a foreground receiver on `con` without waiting for it; returns the marker to wait for."""
    tag = "l%d" % int(time.time() * 1000)
    con.buf = ""
    con.send("%s; rm -f /tmp/rx.bin; %s\n" % (TMO, wrap("tmo 40 socat -u -T 2 %s OPEN:/tmp/rx.bin,creat,trunc >/dev/null 2>&1" % address, tag)))
    time.sleep(2)
    return tag


def received(con, tag):
    return con.wait_for(lambda b: parse(b, tag), 60)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("aredn_image")
    ap.add_argument("host_image")
    ap.add_argument("apks")
    ap.add_argument("--timeout", type=int, default=300)
    args = ap.parse_args()
    failures = []

    def check(name, ok, detail=""):
        print("%s %s %s" % ("PASS" if ok else "FAIL", name, detail.replace("\r\n", " | ")[-300:]))
        if not ok:
            failures.append(name)

    handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=args.apks)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    p6, p4 = free_port(), free_port()
    gw = six = four = None
    try:
        gw = Console(args.aredn_image, extra=["-netdev", "user,id=n1", "-device", "virtio-net-pci,netdev=n1,mac=52:54:00:aa:00:11",
                                              *nic("s6", "52:54:00:aa:00:12", "listen=127.0.0.1:%d" % p6),
                                              *nic("s4", "52:54:00:aa:00:13", "listen=127.0.0.1:%d" % p4)])
        time.sleep(3)  # the gateway must be listening before the hosts connect
        six = Console(args.host_image, extra=nic("s6", "52:54:00:aa:00:21", "connect=127.0.0.1:%d" % p6))
        four = Console(args.host_image, extra=nic("s4", "52:54:00:aa:00:31", "connect=127.0.0.1:%d" % p4))
        for con in (gw, six, four):
            if con.wait_for(lambda b: BOOT_PROMPT in b or None, args.timeout) is None:
                print("FAIL boot")
                return 1
            con.send("\n")
        time.sleep(8)
        res = base.sh(gw, GW_ONLINE.replace("PORT", str(server.server_address[1])), 600)
        text = base.out(res)
        check("stock AREDN loads the Jool apks and the SIIT module", "jool-apk=0" in text and "6.12.94" in text, text)
        for con in (six, four):
            res = base.sh(con, HOST_ONLINE, 900)
            check("host VM has socat, tcpdump and ethtool", "/usr/bin/socat" in base.out(res) and "/usr/bin/tcpdump" in base.out(res), base.out(res))
        push(gw, base.NODE / "usr" / "sbin" / "hams-node-siit", "/usr/sbin/hams-node-siit")
        gw.send("cat > /tmp/export.json <<'HAMS_EOF'\n%s\nHAMS_EOF\n" % base.export())
        res = base.sh(gw, "hams-node-siit render /tmp/export.json /tmp/siit && hams-node-siit apply /tmp/siit; echo apply=$?\njool_siit -i mesha global display | grep pool6", 60)
        check("hams-node-siit loads the instance of the registry export", "apply=0" in base.out(res) and str(siit_map.domain_prefix(base.BLOCK, base.DOMAIN)) in base.out(res), base.out(res))
        for con, setup, mark in ((gw, GW_SETUP, "gw-done"), (six, SIX_SETUP, "six-done"), (four, FOUR_SETUP, "four-done")):
            res = base.sh(con, base.fill(setup), 60)
            check("link addresses and routes set (%s)" % mark, mark in base.out(res), base.out(res))
        for con in (six, four):
            base.sh(con, "seq 1 30000 > /tmp/tx.bin", 30)
            con.send("(tcpdump -l -vv -n -i eth1 > /tmp/cap.txt 2>&1 &)\n")
        time.sleep(2)
        v4_as_v6 = str(siit_map.embed(base.BLOCK, base.DOMAIN, base.V4_HOST))
        res = base.sh(six, "ping -6 -c 5 -W 2 %s >/dev/null 2>&1; echo icmp=$?" % v4_as_v6, 60)
        check("ICMP echo IPv6 host to AREDN host", "icmp=0" in base.out(res), base.out(res))
        if "icmp=0" not in base.out(res):
            for con, cmd in ((six, "ip -6 addr; ip -6 route; ip -6 neigh; ip -s link show eth1"), (gw, "ip -br link; ip -6 neigh; ip -6 route; ip -s link show eth2; jool_siit -i mesha stats display | grep -v ' 0$' | head -20; dmesg | tail -5")):
                print(base.out(base.sh(con, cmd, 30)))
        res = base.sh(four, "ping -c 5 -W 2 %s >/dev/null 2>&1; echo icmp=$?" % base.V6_HOST4, 60)
        check("ICMP echo AREDN host to IPv6 host", "icmp=0" in base.out(res), base.out(res))
        for label, client, server_vm, address, dest, size in (
                ("TCP 60 kB IPv6 to AREDN", six, four, "TCP4-LISTEN:9000,reuseaddr", "TCP6:[%s]:9000" % v4_as_v6, 60000),
                ("TCP 60 kB AREDN to IPv6", four, six, "TCP6-LISTEN:9001,reuseaddr", "TCP4:%s:9001" % base.V6_HOST4, 60000),
                ("UDP IPv6 to AREDN", six, four, "UDP4-LISTEN:9002", "UDP6:[%s]:9002" % v4_as_v6, 800),
                ("UDP AREDN to IPv6", four, six, "UDP6-LISTEN:9003", "UDP4:%s:9003" % base.V6_HOST4, 800)):
            tag = listen(server_vm, address)
            base.sh(client, SEND % (size, dest), 60)
            received(server_vm, tag)
            res = base.sh(server_vm, CHECK % size, 30)
            check("%s, bytes identical" % label, "cmp=0" in base.out(res), base.out(res))
            if "cmp=0" not in base.out(res):
                for con in (client, server_vm):
                    print(base.out(base.sh(con, "ls -l /tmp/rx.bin; grep -E 'Flags|UDP' /tmp/cap.txt | tail -6 | cut -c1-230; ip -s -6 neigh | head -4", 30)))
        time.sleep(1)
        for name, con in (("AREDN host", four), ("IPv6 host", six)):
            res = base.sh(con, "echo bad=$(grep -c -i incorrect /tmp/cap.txt) good=$(grep -c 'cksum .* (correct)' /tmp/cap.txt)", 30)
            found = re.search(r"bad=(\d+) good=(\d+)", base.out(res))
            check("every packet the %s receives has a correct checksum" % name, bool(found) and found.group(1) == "0" and int(found.group(2)) > 20, base.out(res))
        res = base.sh(six, base.fill(base.NEGATIVE).replace("ip netns exec six ", ""), 60)
        check("an address in a domain this gateway does not serve is not translated", "other_domain=1" in base.out(res), base.out(res))
        check("64:ff9b::/96 is not translated", "well_known=1" in base.out(res), base.out(res))
    finally:
        for con in (gw, six, four):
            if con:
                con.close()
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
