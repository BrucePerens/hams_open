# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""
Standalone check script for a `pager.check` record of type "synthetic"
(check.script -> this file's own path). Checks a GitHub fine-grained personal
access token's own expiration date, using GitHub's response header rather
than any endpoint that could reveal or need a broader scope than the token
already has -- any authenticated request returns
`github-authentication-token-expiration` when the token has an expiry set at
all (confirmed live against a real fine-grained PAT, 2026-09-22).

Demonstrates this project's graduated-severity extension to the "synthetic"
check type (see generalized_monitor.py's own `execute_check()`/
`extract_severity_prefix()` comments): as the expiry date gets closer, this
prints an escalating `SEVERITY:` line instead of the flat "high" every other
failing check gets, so an admin sees a low-urgency nudge a month out and a
critical page in the final days -- reusable by any pager_duty deployment
wanting the same behavior for its own expiring credentials, not specific to
GitHub or to hams.com's own token paths (both are overridable).

Reads the token from a file path (default: HAMS_GITHUB_PAT_PATH env var,
falling back to /opt/hams/etc/keys/github_pat_ticket_triage.token) containing
the bare token string, one line, no other formatting.

Exit 0 (healthy) or 1 (failing, at any severity level) with a one-line
message on stdout, matching check_cloudflare_token_expiry.py's own
established convention for a "synthetic" check.
"""

import datetime
import os
import sys
import urllib.error
import urllib.request

from odoo.addons.pager_duty.daemon.generalized_monitor import severity_for_days_left

DEFAULT_TOKEN_PATH = "/opt/hams/etc/keys/github_pat_ticket_triage.token"


# [@ANCHOR: pager_duty:read_github_pat]
def _read_token(token_path):
    with open(token_path, "r", encoding="utf-8") as f:  # audit-ignore-path
        return f.read().strip()


# [@ANCHOR: pager_duty:fetch_github_pat_expiry]
def _fetch_expiry_header(token):
    """A plain GET /user is enough -- the header GitHub returns describes the *token's own*
    expiry, not anything about the account, so this works identically for any fine-grained PAT
    regardless of its granted repository/permission scope."""
    req = urllib.request.Request(
        "https://api.github.com/user",
        headers={"Authorization": f"Bearer {token}", "Accept": "application/vnd.github+json"},
    )
    with urllib.request.urlopen(req, timeout=10) as resp:
        return resp.headers.get("github-authentication-token-expiration")


# [@ANCHOR: pager_duty:github_pat_expiry_main]
def main():
    token_path = os.environ.get("HAMS_GITHUB_PAT_PATH", DEFAULT_TOKEN_PATH)

    try:
        token = _read_token(token_path)
    except OSError as e:
        print(f"cannot read {token_path}: {e}", file=sys.stderr)
        return 1

    try:
        expiration = _fetch_expiry_header(token)
    except (urllib.error.URLError, TimeoutError) as e:
        print(f"GitHub token check failed: {e}", file=sys.stderr)
        return 1

    if not expiration:
        # A real, valid fine-grained PAT can be created with "No expiration date" -- that's not
        # a failure, just nothing to warn about (matches check_cloudflare_token_expiry.py's own
        # identical reasoning for a Cloudflare token with no expiry set).
        print("token verified, no expiry set")
        return 0

    try:
        # GitHub's own format, confirmed live: "2027-09-22 07:00:00 UTC".
        expiry_dt = datetime.datetime.strptime(expiration, "%Y-%m-%d %H:%M:%S %Z").replace(
            tzinfo=datetime.timezone.utc
        )
    except ValueError as e:
        print(f"GitHub returned an unparseable expiration {expiration!r}: {e}", file=sys.stderr)
        return 1

    days_left = (expiry_dt - datetime.datetime.now(datetime.timezone.utc)).days
    severity = severity_for_days_left(days_left)

    if severity:
        print(
            f"SEVERITY:{severity}\nGitHub PAT expires in {days_left} days ({expiration})",
            file=sys.stderr,
        )
        return 1

    print(f"token healthy, expires in {days_left} days ({expiration})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
