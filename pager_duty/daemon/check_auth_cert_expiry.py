# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""
Standalone check script for a `pager.check` record of type "synthetic": warns, then pages, as the
auth.hams.com server certificate (the one the certificate-login gateway serves) approaches expiry.

Why a script and not the built-in "ssl" check type: the same reasons as check_cloudflare_token_expiry.py
for a credential, plus one specific to this certificate. The "ssl" type only ever asks the network; this
certificate is installed from a file (`/etc/hams/auth/auth.crt`) by a renewal job, and the file is the
thing that stops being renewed when the job breaks. This script therefore reads the file first, with
`openssl x509 -enddate`, and only when the file cannot be read (a host where /etc/hams/auth is 0750
root:hams-auth, as provisioning makes it, and this check runs as another account) asks the live server
for its certificate instead, which is also what a visitor would get. No private key is ever read.

Thresholds (Bruce, 2026-10-06): a warning under 30 days, a page under 14. "Page" means the
`high` severity: pager_duty's `low` and `medium` incidents deliberately do not page on-call immediately
(see incident.py, pager_trend_severity_gate), so the shared severity ladder used by the token checks,
which maps 14 days to `medium`, would not page here. This script uses its own ladder:

    under 30 days  low       recorded, not paged
    under 14 days  high      pages on-call
    under  2 days  critical  pages on-call

Exit 0 healthy, 1 failing, with a short message on stdout/stderr and, past the warning line, a bare
`SEVERITY:<level>` line on stderr, which generalized_monitor.py's execute_check() reads (same contract as
the sibling expiry scripts).

Environment overrides (used by this script's tests): HAMS_AUTH_CERT_PATH, HAMS_AUTH_CERT_HOST,
HAMS_AUTH_CERT_PORT, HAMS_AUTH_CERT_WARN_DAYS, HAMS_AUTH_CERT_PAGE_DAYS.
"""

import datetime
import os
import socket
import ssl
import subprocess
import sys

DEFAULT_CERT_PATH = "/etc/hams/auth/auth.crt"
DEFAULT_HOST = "auth.hams.com"
DEFAULT_PORT = 443
DEFAULT_WARN_DAYS = 30
DEFAULT_PAGE_DAYS = 14
CRITICAL_DAYS = 2
OPENSSL = "openssl"


# [@ANCHOR: pager_duty:auth_cert_severity]
def severity_for_auth_cert(days_left, warn_days=DEFAULT_WARN_DAYS, page_days=DEFAULT_PAGE_DAYS):
    """None when healthy; `low` under warn_days (recorded, not paged); `high` under page_days (pages);
    `critical` under CRITICAL_DAYS. days_left may be fractional or negative (already expired)."""
    if days_left < CRITICAL_DAYS:
        return "critical"
    if days_left < page_days:
        return "high"
    if days_left < warn_days:
        return "low"
    return None


# [@ANCHOR: pager_duty:auth_cert_parse_enddate]
def parse_enddate(output):
    """Parses `openssl x509 -enddate` output (`notAfter=Dec 20 03:04:05 2026 GMT`) to an aware UTC datetime."""
    line = output.strip().splitlines()[0] if output.strip() else ""
    if not line.startswith("notAfter="):
        raise ValueError(f"unexpected openssl output: {line[:60]!r}")
    stamp = line.split("=", 1)[1].strip()
    # openssl pads single-digit days with a space ("Jan  5 ..."); strptime's %d accepts that after a space collapse.
    parsed = datetime.datetime.strptime(" ".join(stamp.split()), "%b %d %H:%M:%S %Y %Z")
    return parsed.replace(tzinfo=datetime.timezone.utc)


def _enddate_of(args, pem=None):
    result = subprocess.run(
        [OPENSSL, "x509", "-noout", "-enddate"] + args,
        input=pem, capture_output=True, text=True, timeout=15, check=False,
    )
    if result.returncode != 0:
        raise ValueError(f"openssl could not read the certificate: {result.stderr.strip()[:80]}")
    return parse_enddate(result.stdout)


# [@ANCHOR: pager_duty:auth_cert_read_expiry]
def read_expiry(path, host, port):
    """(expiry, source). The file first; the live server's certificate when the file cannot be read."""
    try:
        with open(path, "rb"):  # audit-ignore-path
            pass
    except OSError:
        pem = ssl.get_server_certificate((host, port), timeout=10)  # no verification: we only read its dates
        return _enddate_of([], pem=pem), f"served by {host}:{port}"
    return _enddate_of(["-in", path]), path


# [@ANCHOR: pager_duty:auth_cert_main]
def main():
    path = os.environ.get("HAMS_AUTH_CERT_PATH", DEFAULT_CERT_PATH)
    host = os.environ.get("HAMS_AUTH_CERT_HOST", DEFAULT_HOST)
    port = int(os.environ.get("HAMS_AUTH_CERT_PORT", DEFAULT_PORT))
    warn_days = int(os.environ.get("HAMS_AUTH_CERT_WARN_DAYS", DEFAULT_WARN_DAYS))
    page_days = int(os.environ.get("HAMS_AUTH_CERT_PAGE_DAYS", DEFAULT_PAGE_DAYS))

    try:
        expiry, source = read_expiry(path, host, port)
    except (ValueError, OSError, ssl.SSLError, socket.timeout, subprocess.SubprocessError) as e:
        # An unreadable certificate is more urgent than a near one: no expiry can be computed at all.
        print("SEVERITY:high", file=sys.stderr)
        print(f"auth certificate unreadable: {e}", file=sys.stderr)
        return 1

    days_left = (expiry - datetime.datetime.now(datetime.timezone.utc)).total_seconds() / 86400.0
    severity = severity_for_auth_cert(days_left, warn_days, page_days)
    if severity:
        print(f"SEVERITY:{severity}", file=sys.stderr)
        print(f"auth.hams.com certificate expires in {days_left:.1f} days ({expiry:%Y-%m-%d}, {source})", file=sys.stderr)
        return 1

    print(f"auth.hams.com certificate healthy, expires in {days_left:.0f} days ({expiry:%Y-%m-%d}, {source})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
