#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""
Standalone check script for a `pager.check` record of type "synthetic": warns, then fails, as the filesystem
that holds a directory of recordings fills, and reports how fast the directory is growing.

Why a script and not the built-in "system" disk check: that check compares only against its `critical` threshold
(`warning_threshold` is exported to the daemon and ignored there), so it cannot warn early. Recordings are kept
forever (nothing deletes in the directory), so the one protection against a full disk is hearing about it in time,
and a growth estimate says how much time that is.

Thresholds (Bruce, 2026-10-13): a warning above 70 percent used, a failure at 85 percent. The warning is the
`medium` severity (recorded, a trend-tracked incident, does not page on-call immediately: see incident.py); the
failure is `high`, which pages (the same ladder as check_auth_cert_expiry.py, for the same reason: `low` and `medium`
do not page). A directory that cannot be measured at all is `high` too.

The percent is `df`'s "Use%": used / (used + available to a non-root user) of the filesystem that holds the directory,
so it is the filesystem, not the directory, that is judged (the directory shares `/` with everything else on hams1).
A directory that does not exist yet is measured through its nearest existing parent, so the check is meaningful on a
host before recording starts.

Growth estimate: the bytes of the regular files directly in the directory and in its `sessions/` folder, with the bytes
modified in the last 7 days giving a per-day rate and, with the filesystem's free space, the days left until the
failure line. A directory younger than 7 days is averaged over its own age (at least one hour), not over the full window,
so the first day does not read as no growth. The listing stops after a time budget and says so rather than make the monitor's 60 second timeout fail
the check. File names are listed, never opened: recordings are not read here.

On a failure the first stderr lines are `SEVERITY:<level>` and a message that carries the essentials in its first
80 characters, because generalized_monitor.py keeps only the first 100 characters of stderr. Healthy runs print the full
estimate line on stdout. Exit 0 healthy, 1 failing.

Usage (the pager.check "script" field, or by hand to read the estimate):
    check_recordings_disk_usage.py [DIRECTORY] [--warn 70] [--fail 85]
Defaults: /var/lib/hams/qso_recordings, 70, 85.
"""

import argparse
import os
import sys
import time

DEFAULT_DIRECTORY = "/var/lib/hams/qso_recordings"
DEFAULT_WARN_PERCENT = 70
DEFAULT_FAIL_PERCENT = 85
RATE_WINDOW_DAYS = 7
SCAN_BUDGET_SECONDS = 20.0
SECONDS_PER_DAY = 86400.0


# [@ANCHOR: pager_duty:recordings_disk_severity]
def severity_for_percent(percent, warn=DEFAULT_WARN_PERCENT, fail=DEFAULT_FAIL_PERCENT):
    """None when `percent` is below `warn`; `medium` from `warn` up to `fail` (recorded, not paged); `high` at or
    above `fail` (pages)."""
    if percent >= fail:
        return "high"
    if percent >= warn:
        return "medium"
    return None


def nearest_existing_directory(path):
    """`path` itself when it is a directory, else its closest existing parent."""
    path = os.path.abspath(path)
    while not os.path.isdir(path):
        parent = os.path.dirname(path)
        if parent == path:
            break
        path = parent
    return path


# [@ANCHOR: pager_duty:recordings_disk_usage]
def filesystem_usage(path):
    """(percent_used, used_bytes, available_bytes) of the filesystem holding `path`, `df`'s way."""
    stat = os.statvfs(nearest_existing_directory(path))
    used = (stat.f_blocks - stat.f_bfree) * stat.f_frsize
    available = stat.f_bavail * stat.f_frsize
    denominator = used + available
    percent = 100.0 * used / denominator if denominator else 100.0
    return percent, used, available


# [@ANCHOR: pager_duty:recordings_volume]
def directory_volume(path, now=None, window_days=RATE_WINDOW_DAYS, budget=SCAN_BUDGET_SECONDS):
    """(file_count, total_bytes, recent_bytes, complete, oldest_mtime) for the regular files directly in `path` and in
    `path/sessions`. `recent_bytes` is the part modified within `window_days`. `complete` is False when the time
    budget ran out. `oldest_mtime` is the oldest modification time seen (None with no files). A directory that does not
    exist or cannot be listed has no files."""
    now = time.time() if now is None else now
    cutoff = now - window_days * SECONDS_PER_DAY
    deadline = time.monotonic() + budget
    count = total = recent = 0
    oldest = None
    for folder in (path, os.path.join(path, "sessions")):
        try:
            with os.scandir(folder) as entries:
                for entry in entries:
                    if time.monotonic() > deadline:
                        return count, total, recent, False, oldest
                    try:
                        if not entry.is_file(follow_symlinks=False):
                            continue
                        info = entry.stat(follow_symlinks=False)
                    except OSError:
                        continue
                    count += 1
                    total += info.st_size
                    if info.st_mtime >= cutoff:
                        recent += info.st_size
                    if oldest is None or info.st_mtime < oldest:
                        oldest = info.st_mtime
        except OSError:
            continue
    return count, total, recent, True, oldest


def human(nbytes):
    value = float(nbytes)
    for unit in ("B", "KB", "MB", "GB", "TB"):
        if value < 1000 or unit == "TB":
            return f"{value:.0f} {unit}" if unit == "B" else f"{value:.1f} {unit}"
        value /= 1000.0
    return f"{value:.1f} TB"  # pragma: no cover


# [@ANCHOR: pager_duty:recordings_estimate]
def estimate(used_bytes, available_bytes, volume, window_days=RATE_WINDOW_DAYS, fail=DEFAULT_FAIL_PERCENT, now=None):
    """(rate_bytes_per_day, days_to_fail). The rate is the bytes modified in the window over the window's length, or
    over the directory's own age (at least one hour) when it is younger than the window. `days_to_fail` is 0.0 when the
    failure line is already crossed and None when the rate is zero."""
    _count, _total, recent, _complete, oldest = volume
    now = time.time() if now is None else now
    span_days = float(window_days)
    if oldest is not None:
        span_days = min(span_days, max((now - oldest) / SECONDS_PER_DAY, 1.0 / 24.0))
    rate = recent / span_days
    # Bytes of growth until used / (used + available) reaches `fail` percent, the denominator `filesystem_usage` uses.
    room = fail / 100.0 * (used_bytes + available_bytes) - used_bytes
    if room <= 0:
        return rate, 0.0
    if rate <= 0:
        return rate, None
    return rate, room / rate


def summary_line(path, percent, volume, rate, days_to_fail, fail=DEFAULT_FAIL_PERCENT):
    count, total, _recent, complete, _oldest = volume
    if days_to_fail is None:
        tail = "no growth seen"
    elif days_to_fail <= 0:
        tail = f"already past {fail:.0f}%"
    elif days_to_fail > 3650:
        tail = f"more than 10 years to {fail:.0f}%"
    else:
        tail = f"about {days_to_fail:.0f} days to {fail:.0f}%"
    partial = "" if complete else " (listing stopped early, partial)"
    return (
        f"recordings in {path}: {count} files, {human(total)}, {human(rate)}/day (up to the last "
        f"{RATE_WINDOW_DAYS} days){partial}; filesystem {percent:.0f}% used, {tail}"
    )


# [@ANCHOR: pager_duty:recordings_disk_main]
def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("directory", nargs="?", default=DEFAULT_DIRECTORY)
    parser.add_argument("--warn", type=float, default=DEFAULT_WARN_PERCENT)
    parser.add_argument("--fail", type=float, default=DEFAULT_FAIL_PERCENT)
    args = parser.parse_args(argv)

    try:
        percent, used, available = filesystem_usage(args.directory)
    except OSError as e:
        print("SEVERITY:high", file=sys.stderr)
        print(f"recordings filesystem unreadable: {e}", file=sys.stderr)
        return 1

    volume = directory_volume(args.directory)
    rate, days_to_fail = estimate(used, available, volume, fail=args.fail)
    line = summary_line(args.directory, percent, volume, rate, days_to_fail, args.fail)
    severity = severity_for_percent(percent, args.warn, args.fail)
    if severity:
        word = "FAILING" if severity == "high" else "warning"
        eta = "" if not days_to_fail else f", {days_to_fail:.0f}d to {args.fail:.0f}%"
        print(f"SEVERITY:{severity}", file=sys.stderr)
        # First 80 characters are what the monitor keeps; the full estimate follows for a person with the journal.
        print(f"recordings fs {percent:.0f}% {word} (warn {args.warn:.0f}/fail {args.fail:.0f}){eta}", file=sys.stderr)
        print(line, file=sys.stderr)
        return 1

    print(line)
    return 0


if __name__ == "__main__":
    sys.exit(main())
