#!/usr/bin/env python3
# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. License: AGPL-3.0.
"""PagerDuty maintenance flag: tells the paging code "a planned restart is under way, do not page", with no Odoo involved.

The flag is one small JSON file, `/etc/pagerduty/maintenance` (override with `PAGERDUTY_MAINTENANCE_FILE`), written by root and
world-readable (mode 0644) so the monitor units, which run with a read-only /etc, can read it. Only root can write it, on
purpose: an unprivileged user must not be able to silence paging.

    {"set_at": <UTC unix time>, "until": <UTC unix time>, "reason": "...", "set_by": "..."}

Rules, all enforced on the READ side so a hand-edited or stale file cannot silence paging for long:
  * It expires by itself. A flag at or past its `until` is ignored, and the next monitor pass removes it.
  * `until` is capped at `set_at` + 2 hours (MAX_MINUTES), whatever the file says.
  * A flag that is malformed, unreadable, has no `set_at`, or whose `set_at` is in the future counts as NOT in
    maintenance. Failing loud (paging) is the safe direction; failing silent is not.

Usage (`start` and `end` need root, or `sudo`; `status` does not; also works over ssh):
    pagerduty-maintenance start [--minutes N] [--reason TEXT]    N defaults to 20, at most 120
    pagerduty-maintenance end
    pagerduty-maintenance status                                 exit 0 when active, 1 when not

The monitors only READ the flag (they run with ProtectSystem=strict and never delete it); `status` and `start` tidy an
expired file. The pager_duty daemon (generalized_monitor.py) imports `flag_path` and `read_flag`. hams_shared/tools/site_monitor.py
must not import from this module, so it carries a read-only copy of the same path, variable name and rules; a test in
pager_duty/tests fails if they disagree. This one file is the whole command: copy it to /usr/local/sbin/pagerduty-maintenance (mode 0755) to install it.
"""
import argparse
import json
import os
import sys
import time

DEFAULT_PATH = "/etc/pagerduty/maintenance"
DEFAULT_MINUTES = 20
MAX_MINUTES = 120
MAX_SECONDS = MAX_MINUTES * 60
CLOCK_SKEW_SECONDS = 300

ACTIVE, EXPIRED, ABSENT, MALFORMED = "active", "expired", "absent", "malformed"


# [@ANCHOR: pager_duty:maintenance_flag_path]
def flag_path(env=None):
    return (os.environ if env is None else env).get("PAGERDUTY_MAINTENANCE_FILE") or DEFAULT_PATH


# [@ANCHOR: pager_duty:maintenance_read_flag]
def read_flag(path=None, now=None):
    """Return a dict with `state` (active/expired/absent/malformed) and, for active/expired, `until` (already capped),
    `reason`, `set_by`, `remaining` seconds. Never raises."""
    path = path or flag_path()
    now = time.time() if now is None else now
    try:
        with open(path, "r", encoding="utf-8") as handle:
            data = json.load(handle)
    except FileNotFoundError:
        return {"state": ABSENT}
    except (OSError, ValueError, UnicodeDecodeError) as exc:
        return {"state": MALFORMED, "error": f"{path}: {exc}"}
    try:
        set_at = float(data["set_at"])
        until = float(data["until"])
        if not (set_at == set_at and until == until) or abs(set_at) == float("inf") or abs(until) == float("inf"):
            raise ValueError("not a finite number")
    except (KeyError, TypeError, ValueError, AttributeError) as exc:
        return {"state": MALFORMED, "error": f"{path}: needs numeric set_at and until ({exc!r})"}
    if set_at > now + CLOCK_SKEW_SECONDS:
        return {"state": MALFORMED, "error": f"{path}: set_at is in the future"}
    until = min(until, set_at + MAX_SECONDS)
    info = {"until": until, "remaining": until - now, "reason": str(data.get("reason", "")),
            "set_by": str(data.get("set_by", ""))}
    info["state"] = ACTIVE if now < until else EXPIRED
    return info


# [@ANCHOR: pager_duty:maintenance_is_active]
def is_active(path=None, now=None):
    return read_flag(path, now)["state"] == ACTIVE


# [@ANCHOR: pager_duty:maintenance_remove_expired]
def remove_expired(path=None, now=None):
    """Delete the flag when it has expired (the commands do this; the monitors never do). Returns True when it was removed. Absent, active and malformed flags are
    left alone (a malformed one is a human's problem to see)."""
    path = path or flag_path()
    if read_flag(path, now)["state"] != EXPIRED:
        return False
    try:
        os.unlink(path)
    except OSError:
        return False
    return True


# [@ANCHOR: pager_duty:maintenance_write_flag]
def write_flag(minutes=DEFAULT_MINUTES, reason="", set_by="", path=None, now=None):
    if not 1 <= minutes <= MAX_MINUTES:
        raise ValueError(f"minutes must be between 1 and {MAX_MINUTES}, not {minutes}")
    path = path or flag_path()
    now = time.time() if now is None else now
    data = {"set_at": int(now), "until": int(now) + int(minutes * 60), "reason": reason, "set_by": set_by}
    directory = os.path.dirname(path)
    os.makedirs(directory, mode=0o755, exist_ok=True)
    tmp = f"{path}.tmp.{os.getpid()}"
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            json.dump(data, handle)
            handle.write("\n")
        os.chmod(tmp, 0o644)
        os.replace(tmp, path)
    finally:
        if os.path.exists(tmp):  # only when the write or the rename failed
            os.unlink(tmp)
    return data


# [@ANCHOR: pager_duty:maintenance_clear_flag]
def clear_flag(path=None):
    """Remove the flag. True when one was there."""
    try:
        os.unlink(path or flag_path())
        return True
    except FileNotFoundError:
        return False


# [@ANCHOR: pager_duty:maintenance_fmt_time]
def _fmt_time(epoch):
    return time.strftime("%Y-%m-%d %H:%M:%S UTC", time.gmtime(epoch))


# [@ANCHOR: pager_duty:maintenance_fmt_remaining]
def _fmt_remaining(seconds):
    seconds = max(0, int(seconds))
    return f"{seconds // 60}m{seconds % 60:02d}s"


# [@ANCHOR: pager_duty:maintenance_describe]
def describe(info):
    return (f"until {_fmt_time(info['until'])} ({_fmt_remaining(info['remaining'])} left)"
            f"{', reason: ' + info['reason'] if info.get('reason') else ''}"
            f"{', set by ' + info['set_by'] if info.get('set_by') else ''}")


# [@ANCHOR: pager_duty:maintenance_main]
def main(argv=None, now=None, out=None):
    out = out or sys.stdout
    parser = argparse.ArgumentParser(prog="pagerduty-maintenance", description="Set, clear or show the pagerduty maintenance flag (pauses paging).")
    sub = parser.add_subparsers(dest="command", required=True)
    start = sub.add_parser("start", help="start a maintenance window")
    start.add_argument("--minutes", type=int, default=DEFAULT_MINUTES, help=f"default {DEFAULT_MINUTES}, max {MAX_MINUTES}")
    start.add_argument("--reason", default="")
    sub.add_parser("end", help="end maintenance now")
    sub.add_parser("status", help="exit 0 when maintenance is active, 1 when not")
    parser.add_argument("--file", default=None, help=f"flag path (default {DEFAULT_PATH} or $PAGERDUTY_MAINTENANCE_FILE)")
    args = parser.parse_args(argv)
    path = args.file or flag_path()

    if args.command == "start":
        if not 1 <= args.minutes <= MAX_MINUTES:
            parser.error(f"--minutes must be between 1 and {MAX_MINUTES}")
        remove_expired(path, now)
        who = os.environ.get("SUDO_USER") or os.environ.get("USER") or "unknown"
        try:
            write_flag(args.minutes, args.reason, who, path, now)
        except OSError as exc:
            print(f"cannot write {path}: {exc} (only root can set it, on purpose)", file=sys.stderr)
            return 2
        print(f"pagerduty maintenance started: {describe(read_flag(path, now))}", file=out)
        return 0
    if args.command == "end":
        try:
            was = clear_flag(path)
        except OSError as exc:
            print(f"cannot remove {path}: {exc} (only root can clear it, on purpose)", file=sys.stderr)
            return 2
        print("pagerduty maintenance ended" if was else "no pagerduty maintenance flag was set", file=out)
        return 0
    info = read_flag(path, now)
    if info["state"] == EXPIRED:
        remove_expired(path, now)  # best effort: an unprivileged `status` cannot, and does not need to
    if info["state"] == ACTIVE:
        print(f"pagerduty maintenance active {describe(info)}", file=out)
        return 0
    if info["state"] == MALFORMED:
        print(f"not in pagerduty maintenance: flag unusable ({info['error']})", file=out)
    elif info["state"] == EXPIRED:
        print(f"not in pagerduty maintenance: flag expired {_fmt_time(info['until'])}", file=out)
    else:
        print("not in pagerduty maintenance", file=out)
    return 1


if __name__ == "__main__":
    sys.exit(main())
