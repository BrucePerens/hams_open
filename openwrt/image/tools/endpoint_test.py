#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-or-later
"""Boot a stock OpenWrt x86-64 image in QEMU (network needed for `apk add openssl-util`), copy hams-node-endpoint in, run it against a stand-in
uclient-fetch that keeps the request body, and check the body's MAC with the portal's formula on the host. Usage: endpoint_test.py IMAGE.
Run through agent_procs.py. Exit 0 = pass."""

import argparse
import base64
import hashlib
import hmac
import json
import pathlib
import sys
import time

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.x25519 import X25519PrivateKey, X25519PublicKey

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from smoke_test import BOOT_PROMPT, Console  # noqa: E402

SBIN = pathlib.Path(__file__).resolve().parents[2] / "hams-node" / "files" / "usr" / "sbin"
RAW = serialization.Encoding.Raw
FETCH = """#!/bin/sh
for a in "$@"; do case $a in --post-data=*) echo "${a#--post-data=}" > /tmp/posted.json ;; esac; done
while [ $# -gt 1 ]; do [ "$1" = -O ] && out=$2; shift; done
echo '{"result":{"status":"success"}}' > "$out"
"""


def push(con, text, remote):
    text = text.replace("\t", "    ")
    con.send("cat > %s <<'HAMS_EOF'\n%s\nHAMS_EOF\n" % (remote, text.rstrip("\n")))
    con.run("chmod 755 %s; wc -c %s" % (remote, remote), 60)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("image")
    ap.add_argument("--timeout", type=int, default=300)
    args = ap.parse_args()
    device, server = X25519PrivateKey.generate(), X25519PrivateKey.generate()
    b = lambda raw: base64.b64encode(raw).decode()  # noqa: E731
    private = b(device.private_bytes(RAW, serialization.PrivateFormat.Raw, serialization.NoEncryption()))
    public = b(device.public_key().public_bytes(RAW, serialization.PublicFormat.Raw))
    server_public = b(server.public_key().public_bytes(RAW, serialization.PublicFormat.Raw))
    # a stock image has the LAN on the first NIC and the WAN on the second: the second one gives the guest internet for apk
    con = Console(args.image, extra=["-netdev", "user,id=n1", "-device", "virtio-net-pci,netdev=n1"])
    failures = []

    def check(name, ok, detail=""):
        print("%s %s %s" % ("PASS" if ok else "FAIL", name, detail))
        if not ok:
            failures.append(name)

    try:
        if con.wait_for(lambda b_: BOOT_PROMPT in b_ or None, args.timeout) is None:
            print("FAIL boot: no console prompt")
            return 1
        con.send("\n")
        time.sleep(8)
        res = con.run("sleep 10; for i in 1 2 3 4 5 6; do apk update >/dev/null 2>&1 && break; sleep 5; done; apk add openssl-util 2>&1 | tail -2; which openssl", 240)
        check("openssl-util installs", bool(res) and "/openssl" in res[0], (res[0] if res else "")[-150:])
        push(con, (SBIN / "hams-node-endpoint").read_text(), "/usr/sbin/hams-node-endpoint")
        push(con, FETCH, "/usr/sbin/uclient-fetch")  # the guest has no portal; the stand-in keeps the body
        # the console's line editor cuts long lines: send short ones
        con.run("mkdir -p /etc/hams-node; echo %s > /etc/hams-node/wg.key; chmod 600 /etc/hams-node/wg.key" % private)
        con.run("touch /etc/config/hams_node; uci set hams_node.main=gateway; uci set hams_node.main.server_key=%s; uci commit hams_node" % server_public)
        con.run("touch /etc/config/hams_node_endpoint; uci set hams_node_endpoint.main=endpoint; uci set hams_node_endpoint.main.host=192.0.2.7; uci commit hams_node_endpoint")
        res = con.run("hams-node-endpoint -u http://127.0.0.1 update; echo rc=$?; cat /tmp/posted.json; cat /etc/hams-node/endpoint_sent", 60)
        out = res[0] if res else ""
        check("update exits 0", "rc=0" in out, out[:300] or con.buf[-800:])
        try:
            body = json.loads(out.split("rc=0")[1].strip().splitlines()[0])["params"]
            shared = server.exchange(X25519PublicKey.from_public_bytes(base64.b64decode(public)))
            expected = hmac.new(shared, ("%s\n%s\n%d" % (public, body["endpoint"], body["timestamp"])).encode(), hashlib.sha256).hexdigest()
            check("device public key and endpoint", body["public_key"] == public and body["endpoint"] == "192.0.2.7:51820")
            check("MAC equals the portal's", body["mac"] == expected, body["mac"])
            check("private key not in the request", private not in out.split("rc=0")[1])
        except (IndexError, KeyError, ValueError) as err:
            check("request body parsed", False, repr(err))
    finally:
        con.close()
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
