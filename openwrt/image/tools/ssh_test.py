#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-or-later
"""hams-node-ssh against the real dropbear of a stock OpenWrt x86-64 image in QEMU (no root). Usage: ssh_test.py IMAGE [--timeout S].
A throwaway ed25519 key is the owner's own key. NOTE: stock OpenWrt dropbear is built without ECDSA (dropbearkey: "Unknown key type 'ecdsa'"), so the member-certificate
ECDSA P-256 keys of hams-node-ssh's marked block cannot log in there (to-do router-ssh-ecdsa-member-key). Checks from the host over a forwarded port: before `apply` the password
logs in; after it only the key does and the password is refused; `off` (unenrol) gives the password back and removes the key. Exit 0 = pass.
Run through agent_procs.py."""

import argparse
import os
import pathlib
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from lan_test import push  # noqa: E402
from smoke_test import BOOT_PROMPT, Console, free_port  # noqa: E402

SCRIPT = pathlib.Path(__file__).resolve().parents[2] / "hams-node" / "files" / "usr" / "sbin" / "hams-node-ssh"
PASSWORD = "test-password-1"
SSH = ["-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null", "-o", "ConnectTimeout=10", "-o", "LogLevel=ERROR"]


def login(port, key=None):
    """True when `ssh root@guest true` succeeds with the key (or, with key None, with the password only)."""
    if key:
        cmd = ["ssh", *SSH, "-o", "PasswordAuthentication=no", "-o", "IdentitiesOnly=yes", "-i", key, "-p", str(port), "root@127.0.0.1", "true"]
    else:
        cmd = ["sshpass", "-p", PASSWORD, "ssh", *SSH, "-o", "PubkeyAuthentication=no", "-p", str(port), "root@127.0.0.1", "true"]
    res = subprocess.run(cmd, capture_output=True, timeout=60)
    if res.returncode:
        print("   ssh rc=%d %s" % (res.returncode, res.stderr.decode("utf-8", "replace").strip()[-200:]))
    return res.returncode == 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("image")
    ap.add_argument("--timeout", type=int, default=300)
    args = ap.parse_args()
    port = free_port()
    failures = []

    def check(name, ok, detail=""):
        print("%s %s %s" % ("PASS" if ok else "FAIL", name, detail))
        if not ok:
            failures.append(name)

    with tempfile.TemporaryDirectory() as tmp:
        key = os.path.join(tmp, "member")
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", key], check=True)
        pub = pathlib.Path(key + ".pub").read_text().strip()
        con = Console(args.image, sshfwd=port)
        try:
            if con.wait_for(lambda b: BOOT_PROMPT in b or None, args.timeout) is None:
                print("FAIL boot: no console prompt")
                return 1
            con.send("\n")
            time.sleep(8)
            push(con, SCRIPT, "/usr/sbin/hams-node-ssh")
            con.run("printf '%s\\n%s\\n' {pw} {pw} | passwd root >/dev/null; /etc/init.d/dropbear restart; sleep 3".format(pw=PASSWORD), 60)
            con.run("ip addr add 10.0.2.15/24 dev br-lan; sleep 2", 30)  # QEMU's forward arrives for this address; br-lan is the LAN zone
            check("password login works before apply", login(port))
            # enrolled, with the owner's own key added the way LuCI does (System > Administration > SSH-Keys)
            con.run("mkdir -p /etc/hams-node /etc/dropbear; echo tok > /etc/hams-node/device_token; echo '%s' > /etc/dropbear/authorized_keys; chmod 600 /etc/dropbear/authorized_keys" % pub, 30)
            res = con.run("hams-node-ssh apply; echo rc=$?; sleep 3", 60)
            check("apply exits 0", bool(res) and "rc=0" in res[0], (res[0] if res else "")[-200:])
            check("key login works after apply", login(port, key))
            check("password login refused after apply", not login(port))
            res = con.run("hams-node-ssh off; echo rc=$?; sleep 3", 60)
            check("off exits 0", bool(res) and "rc=0" in res[0], (res[0] if res else "")[-200:])
            check("password login works again after off", login(port))
            check("the owner's own key still works after off", login(port, key))
        finally:
            con.close()
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
