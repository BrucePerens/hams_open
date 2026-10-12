#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Run `odoo -u <modules>` on a server with exactly the environment the odoo systemd service gets.

    sudo python3 odoo_upgrade.py --db hams_prod --modules hams_helpdesk,ham_relay_bridge [--log /tmp/upgrade.log]
    sudo python3 odoo_upgrade.py --db hams_prod --modules hams_helpdesk --dry-run     # print the command, run nothing

Why it exists (found 2026-10-12): `sudo -u odoo odoo -u ...` gives the upgrade none of the `EnvironmentFile=`
settings of the service (REDIS_USERNAME/REDIS_PASSWORD from /opt/hams/etc/redis.env, the broker and database
credentials, ...). Production Redis refuses unauthenticated clients, so every cache invalidation of the upgrade
failed ("Authentication required", a WARNING) and the Redis cache kept all its pre-upgrade entries.

What it does:
  * reads the service's own `EnvironmentFiles=`, `Environment=`, `User=` and `Group=` from `systemctl show` (so a
    change to the unit's drop-in is followed automatically; nothing is hard-coded) and hands them to a transient
    `systemd-run` unit, which makes systemd (as root) read the files. Their contents are never read into this
    process's output, argv or the log;
  * fails before starting if the service has no EnvironmentFile that defines REDIS_PASSWORD, or the file cannot be
    read (only the NAMES of the variables are looked at, never printed);
  * refuses to run while the odoo service is active (stop it first; the upgrade rewrites tables the live workers use);
  * `distributed_redis_cache`'s invalidate_model_cache independently aborts a install/upgrade at the first cache
    invalidation Redis rejects, so a run that bypasses this tool still fails fast (but only after that point);
  * after the run, exits non-zero if the output contains any Redis authentication failure.

It does not stop or start odoo and does not set the pagerduty maintenance flag: wrap it in the usual
`pagerduty-maintenance start` / `systemctl stop odoo` ... `systemctl start odoo` / `pagerduty-maintenance end`.
"""
# [@ANCHOR: distributed_redis_cache:COMM_odoo_upgrade_tool]

import argparse
import re
import shlex
import subprocess
import sys

SERVICE = "odoo"
ODOO_BIN = "/usr/bin/odoo"
ODOO_CONF = "/etc/odoo/odoo.conf"
AUTH_FAILURE = re.compile(r".*(Authentication required|invalid username-password|NOAUTH)", re.I)
MODULE_NAME = re.compile(r"^[a-z0-9_]+$")
REDIS_SECRET_KEY = "REDIS_PASSWORD"


class UpgradeError(Exception):
    """A precondition failed; the message is safe to print (it never contains a value from an env file)."""


def parse_service(show_output):
    """Parse `systemctl show odoo -p EnvironmentFiles -p Environment -p User -p Group` into
    {"files": [(path, optional)], "env": [KEY=VALUE...], "user": str, "group": str}."""
    files, env, user, group = [], [], "", ""
    for line in show_output.splitlines():
        key, _, value = line.partition("=")
        if key == "EnvironmentFiles":
            match = re.match(r"^(\S+) \(ignore_errors=(yes|no)\)$", value.strip())
            if match:
                files.append((match.group(1), match.group(2) == "yes"))
        elif key == "Environment":
            env.extend(shlex.split(value))
        elif key == "User":
            user = value.strip()
        elif key == "Group":
            group = value.strip()
    return {"files": files, "env": env, "user": user, "group": group}


def env_file_keys(text):
    """The variable NAMES an EnvironmentFile defines (values are discarded immediately)."""
    keys = set()
    for line in text.splitlines():
        line = line.strip()
        if line and not line.startswith(("#", ";")) and "=" in line:
            keys.add(line.partition("=")[0].strip())
    return keys


def require_redis_credentials(service, read_text):
    """Raise UpgradeError unless some EnvironmentFile of the service defines REDIS_PASSWORD."""
    if not service["files"]:
        raise UpgradeError(f"the {SERVICE} service has no EnvironmentFile=; refusing to run an upgrade without its credentials")
    for path, _optional in service["files"]:
        try:
            if REDIS_SECRET_KEY in env_file_keys(read_text(path)):
                return
        except OSError as e:
            raise UpgradeError(f"cannot read {path} ({e.__class__.__name__}); run as root") from e
    raise UpgradeError(f"none of the {SERVICE} service's EnvironmentFiles defines {REDIS_SECRET_KEY}; "
                       "production Redis would reject the upgrade's cache invalidations")


def build_command(service, db, modules):
    """The systemd-run command line. Environment files are passed by PATH, so systemd (root) reads them."""
    if not service["user"]:
        raise UpgradeError(f"cannot determine the {SERVICE} service's User=")
    cmd = ["systemd-run", "--quiet", "--wait", "--pipe", "--collect", "--service-type=exec",
           "-p", f"User={service['user']}"]
    if service["group"]:
        cmd += ["-p", f"Group={service['group']}"]
    for path, optional in service["files"]:
        cmd += ["-p", f"EnvironmentFile={'-' if optional else ''}{path}"]
    for assignment in service["env"]:
        cmd += ["-E", assignment]
    return cmd + [ODOO_BIN, "--config", ODOO_CONF, "-d", db, "-u", ",".join(modules), "--stop-after-init"]


def run(argv, show=None, read_text=None, execute=None, is_active=None, out=None):
    """Returns the process exit code. Dependencies are injectable for tests."""
    out = out or sys.stdout
    args = parse_args(argv)
    modules = [m for m in args.modules.split(",") if m]
    bad = [m for m in modules if not MODULE_NAME.match(m)]
    if not modules or bad:
        raise UpgradeError(f"invalid --modules {args.modules!r}")
    if not re.match(r"^[A-Za-z0-9_]+$", args.db):
        raise UpgradeError(f"invalid --db {args.db!r}")
    show = show or (lambda: subprocess.run(
        ["systemctl", "show", SERVICE, "-p", "EnvironmentFiles", "-p", "Environment", "-p", "User", "-p", "Group"],
        check=True, capture_output=True, text=True).stdout)
    read_text = read_text or (lambda p: open(p, encoding="utf-8").read())
    is_active = is_active or (lambda: subprocess.run(["systemctl", "is-active", "--quiet", SERVICE]).returncode == 0)
    service = parse_service(show())
    require_redis_credentials(service, read_text)
    if not args.dry_run and is_active():
        raise UpgradeError(f"the {SERVICE} service is running; `systemctl stop {SERVICE}` first (and wrap in pagerduty-maintenance)")
    cmd = build_command(service, args.db, modules)
    print("running: " + shlex.join(cmd), file=out)
    if args.dry_run:
        return 0
    return (execute or _execute)(cmd, args.log, out)


def _execute(cmd, log_path, out):
    auth_failures = 0
    log = open(log_path, "w", encoding="utf-8") if log_path else None
    try:
        proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        for line in proc.stdout:
            if AUTH_FAILURE.match(line):
                auth_failures += 1
            (log or out).write(line)
        rc = proc.wait()
    finally:
        if log:
            log.close()
    if auth_failures:
        print(f"FAILED: {auth_failures} Redis authentication failure(s) during the upgrade; the Redis cache was NOT "
              "invalidated. Delete the hams_prod:distributed_cache:* keys and restart odoo.", file=sys.stderr)
        return rc or 3
    return rc


def parse_args(argv):
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("--db", required=True)
    p.add_argument("--modules", required=True, help="comma-separated module names")
    p.add_argument("--log", help="write odoo's output here instead of stdout")
    p.add_argument("--dry-run", action="store_true", help="print the command and exit")
    return p.parse_args(argv)


def main():
    try:
        return run(sys.argv[1:])
    except UpgradeError as e:
        print(f"refusing: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
