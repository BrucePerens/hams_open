#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-or-later
"""Boot the test image in QEMU (no root), check packages, services and the LuCI page over the serial console.

Usage: smoke_test.py IMAGE [--timeout SECONDS]. Run it through agent_procs.py. Exit 0 = all checks passed.
"""

import argparse
import os
import queue
import re
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


class Console:
    def __init__(self, image):
        accel = ["-enable-kvm", "-cpu", "host"] if os.access("/dev/kvm", os.W_OK) else ["-cpu", "max"]
        cmd = ["qemu-system-x86_64", *accel, "-m", "512", "-smp", "2", "-display", "none", "-monitor", "none",
               "-serial", "stdio", "-drive", "file=%s,format=raw,if=virtio,snapshot=on" % image,
               "-netdev", "user,id=n0", "-device", "virtio-net-pci,netdev=n0"]
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
                pass
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
    con = Console(args.image)
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
        res = con.run("apk list -I 2>&1 | grep -E '^(hams-node|luci-app-hams-node)-'")
        out = res[0] if res else ""
        check("packages installed", bool(res) and res[1] == 0 and "hams-node-" in out and "luci-app-hams-node-" in out, out.strip())
        res = con.run("/etc/init.d/hams-node enable; /etc/init.d/hams-node start; echo s=$?; /etc/init.d/hams-node enabled; echo e=$?; "
                      "/etc/init.d/uhttpd running; echo u=$?; ls /usr/sbin/hams-node-siit /usr/sbin/hams-node-wg")
        out = res[0] if res else ""
        check("hams-node starts (no export_url: a no-op) and is enabled, uhttpd runs", bool(res) and "s=0" in out and "e=0" in out and "u=0" in out, out.strip().replace("\r\n", " | ")[-200:])
        res = con.run("ubus call hams-node status", 30)
        check("rpcd hams-node status answers", bool(res) and res[1] == 0, (res[0] if res else "").strip()[:120])
        # LuCI answers its login page with 403 to an unauthenticated request; uclient-fetch reports the status
        res = con.run("uclient-fetch -O /dev/null http://127.0.0.1/cgi-bin/luci/ 2>&1", 30)
        out = res[0] if res else ""
        check("LuCI answers (login page)", bool(res) and ("403" in out or "Download completed" in out), out.strip()[:300])
        res = con.run("uclient-fetch -qO- http://127.0.0.1/luci-static/resources/view/hams-node/status.js | head -c 200", 30)
        check("LuCI hams-node view served", bool(res) and res[1] == 0 and len(res[0].strip()) > 50)
    finally:
        con.close()
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
