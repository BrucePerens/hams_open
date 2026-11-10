#!/usr/bin/env python3
# -*- coding: utf-8 -*-
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Set up the Tier B secret broker on any systemd host. Run as root:

    python3 provision.py --web-user odoo --code-dir /path/to/zero_sudo/daemon [--odoo-unit odoo.service] [--dry-run]

What it does (each step is printed; `--dry-run` prints and changes nothing):

1. System account `hams_secret_broker` (no home, no shell) and group `hams_secret_broker_clients`; the web
   account is added to the clients group, which can reach the socket directory and nothing else.
2. `/etc/hams_secret_broker/` root:root 0755 with `broker.json` (policy only, no secret; written only if absent)
   and `secrets` root:root 0600 (created empty only if absent; never overwritten, never read by this script).
   The broker reads it through `LoadCredential=`, so neither the broker account nor the web account can read
   the file itself.
3. `hams-secret-broker.service`, with `RuntimeDirectory=hams_secret_broker` (0750, group clients) for the socket
   and the usual sandboxing.
4. With `--odoo-unit`, a drop-in for that unit setting `HAMS_SECRET_STORE=broker:<socket>` and adding the clients
   group to it.

Then: put `name=value` lines in the secrets file with `sudoedit`, list each secret's policy in `broker.json`, and
`systemctl enable --now hams-secret-broker`. Docs: zero_sudo/docs/stories/secret_store_tier_b.md.
"""

import argparse
import json
import os
import pwd
import subprocess
import sys

ACCOUNT = "hams_secret_broker"
CLIENTS_GROUP = "hams_secret_broker_clients"
ETC_DIR = "/etc/hams_secret_broker"
SOCKET = "/run/hams_secret_broker/broker.sock"
UNIT_NAME = "hams-secret-broker.service"


# [@ANCHOR: zero_sudo:secret_broker_provision_unit_text]
def unit_text(code_dir, python="/usr/bin/python3"):
    """The systemd unit of the broker. The secrets file reaches it only as a credential."""
    return f"""[Unit]
Description=hams secret broker (Tier B: operations on secrets the web process cannot read)
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User={ACCOUNT}
Group={CLIENTS_GROUP}
ExecStart={python} {code_dir}/secret_broker/main.py --config {ETC_DIR}/broker.json
LoadCredential=secrets:{ETC_DIR}/secrets
RuntimeDirectory=hams_secret_broker
RuntimeDirectoryMode=0750
UMask=0117
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6
RestrictSUIDSGID=yes
LockPersonality=yes
MemoryDenyWriteExecute=yes
CapabilityBoundingSet=
SystemCallArchitectures=native
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
"""


# [@ANCHOR: zero_sudo:secret_broker_provision_odoo_dropin_text]
def odoo_dropin_text():
    """The drop-in for the Odoo unit: the store spec and the clients group. Nothing secret."""
    return f"""[Service]
SupplementaryGroups={CLIENTS_GROUP}
Environment=HAMS_SECRET_STORE=broker:{SOCKET}
"""


# [@ANCHOR: zero_sudo:secret_broker_provision_default_config]
def default_config(web_user):
    """The starting broker.json: the web account may ask and must not read; no secret is configured
    yet, so the broker refuses to start until the operator adds one (fail fast)."""
    return {
        "socket": SOCKET,
        "allowed_users": [web_user],
        "deny_readers": [web_user],
        "credentials": "systemd",
        "secrets": {},
    }


# [@ANCHOR: zero_sudo:secret_broker_provision_plan]
def plan(web_user, code_dir, odoo_unit=None, python="/usr/bin/python3"):
    """The ordered steps, as tuples:
    ("run", argv) a command; ("dir", path, mode) a root-owned directory;
    ("file_if_absent", path, mode, text) a root-owned file written only when it does not exist;
    ("file", path, mode, text) a root-owned file (re)written, used for unit files only."""
    steps = [
        ("run", ["groupadd", "--system", "--force", CLIENTS_GROUP]),
        ("run_if_no_user", ["useradd", "--system", "--no-create-home", "--shell", "/usr/sbin/nologin",
                            "--gid", CLIENTS_GROUP, ACCOUNT]),
        ("run", ["usermod", "--append", "--groups", CLIENTS_GROUP, web_user]),
        ("dir", ETC_DIR, 0o755),
        ("file_if_absent", f"{ETC_DIR}/broker.json", 0o644, json.dumps(default_config(web_user), indent=2) + "\n"),
        ("file_if_absent", f"{ETC_DIR}/secrets", 0o600,
         "# name=value, one per line. Root-only; the broker gets it through LoadCredential=.\n"),
        ("file", f"/etc/systemd/system/{UNIT_NAME}", 0o644, unit_text(code_dir, python)),
    ]
    if odoo_unit:
        steps.append(("file", f"/etc/systemd/system/{odoo_unit}.d/hams_secret_broker.conf", 0o644,
                      odoo_dropin_text()))
    steps.append(("run", ["systemctl", "daemon-reload"]))
    return steps


# [@ANCHOR: zero_sudo:secret_broker_provision_account_exists]
def account_exists(name):
    """True when the system has an account called `name`."""
    return any(entry.pw_name == name for entry in pwd.getpwall())


# [@ANCHOR: zero_sudo:secret_broker_provision_apply]
def apply(steps, dry_run, run=subprocess.run, user_exists=account_exists):
    """Carry out `steps` (see plan), printing each. Files are root-owned, written with the stated mode
    from the start; an existing `file_if_absent` target is left exactly as it is."""
    for step in steps:
        kind = step[0]
        if kind in ("run", "run_if_no_user"):
            if kind == "run_if_no_user" and user_exists(step[1][-1]):
                print(f"keep: account {step[1][-1]} exists")
                continue
            print("run:", " ".join(step[1]))
            if not dry_run:
                run(step[1], check=True)
        elif kind == "dir":
            print(f"dir: {step[1]} root:root {oct(step[2])}")
            if not dry_run:
                os.makedirs(step[1], exist_ok=True)
                os.chown(step[1], 0, 0)
                os.chmod(step[1], step[2])
        else:
            path, mode, text = step[1], step[2], step[3]
            if kind == "file_if_absent" and os.path.lexists(path):
                print(f"keep: {path} exists")
                continue
            print(f"write: {path} root:root {oct(mode)}")
            if not dry_run:
                os.makedirs(os.path.dirname(path), exist_ok=True)
                flags = os.O_WRONLY | os.O_CREAT | os.O_NOFOLLOW | (os.O_EXCL if kind == "file_if_absent" else os.O_TRUNC)
                fd = os.open(path, flags, mode)
                with os.fdopen(fd, "w", encoding="utf-8") as handle:
                    handle.write(text)
                os.chown(path, 0, 0)
                os.chmod(path, mode)


# [@ANCHOR: zero_sudo:secret_broker_provision_main]
def main(argv=None):
    """Command line; refuses to change anything unless run as root (a dry run works as anyone)."""
    parser = argparse.ArgumentParser(description="Provision the Tier B secret broker")
    parser.add_argument("--web-user", required=True, help="the account the Odoo web process runs as")
    parser.add_argument("--code-dir", required=True, help="the zero_sudo/daemon directory of the installed module")
    parser.add_argument("--odoo-unit", help="the Odoo systemd unit to give a drop-in, e.g. odoo.service")
    parser.add_argument("--python", default="/usr/bin/python3")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args(argv)
    if not args.dry_run and os.geteuid() != 0:
        print("run as root (or use --dry-run)", file=sys.stderr)
        return 2
    apply(plan(args.web_user, os.path.abspath(args.code_dir), args.odoo_unit, args.python), args.dry_run)
    return 0


if __name__ == "__main__":
    sys.exit(main())
