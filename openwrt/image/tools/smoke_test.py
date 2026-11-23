#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-or-later
"""Boot the test image in QEMU (no root), check packages, services and the LuCI page over the serial console.

Usage: smoke_test.py IMAGE [--timeout SECONDS]. Run it through agent_procs.py. Exit 0 = all checks passed.
"""

import argparse
import os
import queue
import re
import socket
import subprocess
import sys
import threading
import time

BOOT_PROMPT = "Please press Enter to activate this console"


def wrap(cmd, tag):
    """Shell line that prints markers around cmd's output and its exit code (the echo of this line cannot match)."""
    return "printf 'BEGIN-%%s\\n' %s; %s; printf 'END-%%s-%%d\\n' %s $?" % (tag, cmd, tag)


def parse(text, tag):
    m = re.search(r"BEGIN-%s\r?\n(.*?)END-%s-(\d+)" % (re.escape(tag), re.escape(tag)), text, re.S)
    if not m:
        return None
    return m.group(1), int(m.group(2))


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def http_answers(port, timeout=8):
    """True when an HTTP request to 127.0.0.1:port gets any reply (QEMU's host forward accepts the connection even when the guest
    firewall drops it, so a connect alone proves nothing: only a reply does)."""
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=timeout) as c:
            c.settimeout(timeout)
            c.sendall(b"GET / HTTP/1.0\r\n\r\n")
            return bool(c.recv(64))
    except OSError:
        return False


# Guest-side setup for the node-network checks, as short lines (the serial console takes about 1000 characters per line). The BGP secret
# and the .ham resolver (a second dnsmasq on 127.0.0.2:5353 that answers x.ham and logs every query) are throwaway test values.
BIRD_SETUP = [
    "mkdir -p /etc/hams-node; echo testsecret > /etc/hams-node/bird.secret; chmod 600 /etc/hams-node/bird.secret",
    "uci set hams_node_bird.main.enabled=1; uci set hams_node_bird.main.asn=4200000001; uci set hams_node_bird.main.serial=16909060",
    "uci set hams_node_bird.main.prefix=2001:db8:ab00::/40; uci set hams_node_bird.main.prefix60=2001:db8:ab00:10::/60",
    "uci add_list hams_node_bird.main.server=2001:db8:ab00::1; uci commit hams_node_bird",
    "/etc/init.d/hams-node-bird start; echo st=$?; sleep 3",
    "bird -p -c /etc/hams-node/bird.conf; echo parse=$?",
    "test -S /var/run/hams-node-bird.ctl; echo sock=$?",  # the control socket of our own instance (the bird2 package runs its own too)
]
DNS_SETUP = [
    "dnsmasq --conf-file=/dev/null --port=5353 --listen-address=127.0.0.2 --bind-interfaces --no-resolv --no-hosts"
    " --address=/x.ham/192.0.2.9 --log-queries --log-facility=/tmp/stub.log --pid-file=/tmp/stub.pid; sleep 1",
    "uci set hams_node_dns.main.enabled=1; uci add_list hams_node_dns.main.resolver=127.0.0.2#5353",
    "uci set hams_node_dns.main.prefix=2001:db8:ab00::/40; uci commit hams_node_dns",
    "/etc/init.d/hams-node-dns start; echo st=$?; sleep 3",
    "nslookup x.ham 127.0.0.1 2>&1; nslookup public-name.example 127.0.0.1 >/dev/null 2>&1; true",
]
# The lan network moves into the ham zone (the VM's one NIC is lan), so the host's forwarded connection arrives from the ham zone:
# closed until port 80 is opened. rf0 is set to rf, so WireGuard must be refused on it.
FW_SETUP = [
    "uci set hams_node_fw.main.enabled=1; uci add_list hams_node_fw.main.iface=lan; uci add_list hams_node_fw.main.rf_iface=rf0",
    "uci commit hams_node_fw; uci del_list firewall.@zone[0].network=lan; uci commit firewall",
    "/etc/init.d/hams-node-fw start; echo st=$?; sleep 3; nft list chain inet fw4 input_ham 2>&1 | head -20",
]
FW_OPEN = [
    "uci add_list hams_node_fw.main.open=tcp:80; uci commit hams_node_fw; /etc/init.d/hams-node-fw start; echo st=$?; sleep 3",
]
RF_WG = [
    "echo 'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA= 192.0.2.7:51820 2001:db8:ab01::/48' > /tmp/peers",
    "hams-node-wg apply rf0 internet /tmp/peers 2>&1; echo rc=$?",
]


def run_all(con, cmds, timeout=60):
    """Run each command line; the joined output, or None if one of them never completed."""
    out = []
    for cmd in cmds:
        res = con.run(cmd, timeout)
        if res is None:
            return None
        out.append(res[0])
    return "\n".join(out)


class Console:
    def __init__(self, image, hostfwd=None):
        accel = ["-enable-kvm", "-cpu", "host"] if os.access("/dev/kvm", os.W_OK) else ["-cpu", "max"]
        cmd = ["qemu-system-x86_64", *accel, "-m", "512", "-smp", "2", "-display", "none", "-monitor", "none",
               "-serial", "stdio", "-drive", "file=%s,format=raw,if=virtio,snapshot=on" % image,
               "-netdev", "user,id=n0" + (",hostfwd=tcp:127.0.0.1:%d-:80" % hostfwd if hostfwd else ""), "-device", "virtio-net-pci,netdev=n0"]
        self.proc = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        self.buf = ""
        self.q = queue.Queue()
        threading.Thread(target=self._read, daemon=True).start()

    def _read(self):
        while True:
            chunk = self.proc.stdout.read1(4096)
            if not chunk:
                return
            self.q.put(chunk.decode("utf-8", "replace"))

    def wait_for(self, pred, timeout):
        end = time.time() + timeout
        while time.time() < end:
            res = pred(self.buf)
            if res is not None and res is not False:
                return res
            try:
                self.buf += self.q.get(timeout=1)
            except queue.Empty:
                continue
        return None

    def send(self, text):
        self.proc.stdin.write(text.encode())
        self.proc.stdin.flush()

    def run(self, cmd, timeout=60):
        tag = "t%d" % int(time.time() * 1000)
        self.buf = ""
        self.send(wrap(cmd, tag) + "\n")
        return self.wait_for(lambda b: parse(b, tag), timeout)

    def close(self):
        # the guest is disposable (snapshot=on): ask QEMU to exit through the guest, then reap our own child
        try:
            self.send("poweroff -f\n")
            self.proc.wait(timeout=20)
        except (subprocess.TimeoutExpired, OSError):
            self.proc.terminate()
            self.proc.wait(timeout=20)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("image")
    ap.add_argument("--timeout", type=int, default=300, help="seconds to wait for the boot")
    args = ap.parse_args()
    port = free_port()
    con = Console(args.image, port)
    failures = []

    def check(name, ok, detail=""):
        print("%s %s %s" % ("PASS" if ok else "FAIL", name, detail))
        if not ok:
            failures.append(name)

    try:
        if con.wait_for(lambda b: BOOT_PROMPT in b or None, args.timeout) is None:
            print("FAIL boot: no console prompt within %ds" % args.timeout)
            return 1
        con.send("\n")
        time.sleep(2)
        con.run("for i in $(seq 40); do /etc/init.d/uhttpd running && break; sleep 3; done; sleep 5", 200)  # boot settle
        res = con.run("apk list -I 2>&1 | grep -E '^(hams-node|hams-node-bird|hams-node-dns|hams-node-fw|luci-app-hams-node)-'")
        out = res[0] if res else ""
        check("packages installed", bool(res) and res[1] == 0 and "hams-node-" in out and "luci-app-hams-node-" in out
              and all("hams-node-%s-" % n in out for n in ("bird", "dns", "fw")), out.strip())
        res = con.run("/etc/init.d/hams-node enable; /etc/init.d/hams-node start; echo s=$?; /etc/init.d/hams-node enabled; echo e=$?; "
                      "/etc/init.d/uhttpd running; echo u=$?; ls /usr/sbin/hams-node-siit /usr/sbin/hams-node-wg")
        out = res[0] if res else ""
        check("hams-node starts (no export_url: a no-op) and is enabled, uhttpd runs", bool(res) and "s=0" in out and "e=0" in out and "u=0" in out, out.strip().replace("\r\n", " | ")[-200:])
        res = con.run("ubus call hams-node status", 30)
        check("rpcd hams-node status answers", bool(res) and res[1] == 0, (res[0] if res else "").strip()[:120])
        # LuCI answers its login page with 403 to an unauthenticated request; uclient-fetch reports the status
        res = con.run("uclient-fetch -O /dev/null http://$(uci get network.loopback.ipaddr | cut -d/ -f1)/cgi-bin/luci/ 2>&1", 30)
        out = res[0] if res else ""
        check("LuCI answers (login page)", bool(res) and ("403" in out or "Download completed" in out), out.strip()[:300])
        res = con.run("uclient-fetch -qO- http://$(uci get network.loopback.ipaddr | cut -d/ -f1)/luci-static/resources/view/hams-node/status.js | head -c 200", 30)
        check("LuCI hams-node view served", bool(res) and res[1] == 0 and len(res[0].strip()) > 50)
        out = run_all(con, BIRD_SETUP)
        check("BIRD: config parses (bird -p) and our instance runs", out is not None and "st=0" in out and "parse=0" in out
              and "sock=0" in out, (out or "").strip().replace("\r\n", " | ")[-200:])
        out = run_all(con, DNS_SETUP)
        stub = con.run("cat /tmp/stub.log", 15)
        log = stub[0] if stub else ""
        check("dnsmasq: .ham query is forwarded to the stub and answered", out is not None and "192.0.2.9" in out,
              (out or "").strip().replace("\r\n", " | ")[-200:])
        check("dnsmasq: public names are not sent to the ham resolver", "x.ham" in log and "public-name.example" not in log, log.strip()[-200:])
        res = con.run("ip addr add 10.0.2.15/24 dev br-lan; echo ok", 15)  # where QEMU's host forward delivers
        check("firewall: before the zones load, the guest answers HTTP", bool(res) and http_answers(port))
        out = run_all(con, FW_SETUP)
        check("firewall: ham zone loads", out is not None and "st=0" in out and "chain input_ham" in out, (out or "").strip().replace("\r\n", " | ")[-200:])
        check("firewall: inbound from the ham zone is dropped by default", not http_answers(port))
        out = run_all(con, FW_OPEN)
        check("firewall: opening tcp 80 in uci opens it", out is not None and "st=0" in out and http_answers(port))
        out = run_all(con, RF_WG, 30)
        check("rf interface refuses WireGuard", out is not None and "rc=1" in out and "refusing WireGuard" in out, (out or "").strip()[:200])
    finally:
        con.close()
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
