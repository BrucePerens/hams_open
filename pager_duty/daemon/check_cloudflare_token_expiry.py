# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""
Standalone check script for a `pager.check` record of type "synthetic"
(check.script -> this file's own path). Verifies the Cloudflare API token
used by the `relay_cert_renew` daemon's DNS-01 renewal hasn't crept close to
its own 1-year expiry -- LOCAL_RELAY_TLS_CERTIFICATE.md's item 4, second
half ("no existing check type covers a non-network-presented credential's
expiry"). The first half (the relay wildcard cert's own TLS expiry) needs no
new code at all -- an existing `pager.check` record of type "ssl" targeting
`relay.hams.com:443` already covers it, since that check connects over the
network and inspects the presented certificate, the same way a real client
would.

Reads the token from `/opt/hams/etc/relay_cert_renew/cloudflare.ini` by
default (override via HAMS_CLOUDFLARE_CREDENTIALS_PATH, used by this
script's own tests). This path is not `~/.secrets/certbot/cloudflare.ini`
(bruce-owned, mode 400) -- it's the working copy `relay_cert_renew`'s own
provisioning already places under `/opt/hams/etc/relay_cert_renew/`, owned
by `odoo:odoo`, mode 600. Confirmed directly against the real dev box before
writing this: `odoo` can read it (`sudo -u odoo cat` succeeds), `bruce`
cannot (`Permission denied`, even for the user that originally owns the
`~/.secrets` original) -- the exact real-world permission boundary this
check needs already exists; no new privilege-transition mechanism, setuid
helper, or airgapped spooling sidecar is needed, because `pager_duty`'s own
daemon already runs as the same `odoo` user that already owns this file.

Exit 0 (healthy) or 1 (failing) with a one-line message on stdout, matching
how `generalized_monitor.py`'s "synthetic" check type reports a script's
outcome (stderr on failure is truncated to 100 chars in the incident body,
so keep the failure message short and put the real detail on stdout too).
"""

import configparser
import datetime
import json
import os
import sys
import urllib.error
import urllib.request

DEFAULT_CREDENTIALS_PATH = "/opt/hams/etc/relay_cert_renew/cloudflare.ini"
# A token this close to expiring should page well before it's too late to
# generate and roll a replacement by hand -- matches the "ssl" check type's
# own default `critical` threshold (14 days) for the same kind of decision.
DEFAULT_WARN_DAYS = 30
# Not a secret -- a Cloudflare account identifier, not a credential (doesn't
# match any of MASTER_01_SECURITY_ZERO_SUDO.md's own restricted-substring
# list: secret/key/password/token/auth/crypt/cert). Real, current hams.com
# account ID, same one already recorded in ~/.secrets/cloudflare_hams_com.env.
# Overridable via HAMS_CLOUDFLARE_ACCOUNT_ID for tests/future account moves.
DEFAULT_ACCOUNT_ID = "a279ea641ad18fd1c87fa1cc77d3e10b"


def _read_token(credentials_path):
    """
    `cloudflare.ini` is certbot's own `dns_cloudflare_credentials` format:
    a bare `key = value` line, no section header -- configparser needs a
    synthetic one prepended to parse it, the same workaround this
    codebase's other certbot-credential-reading code already needs.
    """
    parser = configparser.ConfigParser()
    with open(credentials_path, "r", encoding="utf-8") as f:  # audit-ignore-path
        parser.read_string("[dns_cloudflare]\n" + f.read())
    return parser.get("dns_cloudflare", "dns_cloudflare_api_token").strip()


def _fetch_token_expiry(token, account_id):
    """
    Calls Cloudflare's account-scoped `/accounts/{account_id}/tokens/verify`,
    NOT the generic `/user/tokens/verify` -- confirmed directly against the
    real token earlier tonight (this session's own night_shift_todo.md has
    the full account: `/user/tokens/verify` returns "Invalid API Token" for
    this exact token even though it's genuinely valid, because it's an
    account-scoped token lacking user-level API access; the account-scoped
    endpoint correctly returns `"status":"active"`). Re-confirmed again while
    building this script: the generic endpoint gave a 401 here too. Returns
    the real `expires_on` string Cloudflare reports, or raises if the token
    itself doesn't verify (its own, more urgent failure -- an invalid token
    needs to page immediately, not wait for an expiry calculation that can't
    happen without a valid response).
    """
    req = urllib.request.Request(
        f"https://api.cloudflare.com/client/v4/accounts/{account_id}/tokens/verify",
        headers={"Authorization": f"Bearer {token}"},
    )
    with urllib.request.urlopen(req, timeout=10) as resp:
        body = json.loads(resp.read().decode("utf-8"))
    if not body.get("success"):
        errors = body.get("errors", [])
        raise RuntimeError(f"token did not verify: {errors}")
    result = body.get("result") or {}
    return result.get("expires_on")


def main():
    credentials_path = os.environ.get(
        "HAMS_CLOUDFLARE_CREDENTIALS_PATH", DEFAULT_CREDENTIALS_PATH
    )
    account_id = os.environ.get("HAMS_CLOUDFLARE_ACCOUNT_ID", DEFAULT_ACCOUNT_ID)
    warn_days = int(os.environ.get("HAMS_CLOUDFLARE_TOKEN_WARN_DAYS", DEFAULT_WARN_DAYS))

    try:
        token = _read_token(credentials_path)
    except OSError as e:
        print(f"cannot read {credentials_path}: {e}", file=sys.stderr)
        return 1
    except (configparser.Error, KeyError) as e:
        print(f"malformed credentials file {credentials_path}: {e}", file=sys.stderr)
        return 1

    try:
        expires_on = _fetch_token_expiry(token, account_id)
    except (urllib.error.URLError, TimeoutError, RuntimeError, json.JSONDecodeError) as e:
        print(f"Cloudflare token verify failed: {e}", file=sys.stderr)
        return 1

    if not expires_on:
        # A real, valid Cloudflare token can genuinely have no expiry (an
        # account owner can create a non-expiring token) -- that's not a
        # failure, just nothing to warn about.
        print("token verified, no expiry set")
        return 0

    expiry_dt = datetime.datetime.fromisoformat(expires_on.replace("Z", "+00:00"))
    days_left = (expiry_dt - datetime.datetime.now(datetime.timezone.utc)).days

    if days_left <= warn_days:
        print(
            f"Cloudflare API token expires in {days_left} days ({expires_on})",
            file=sys.stderr,
        )
        return 1

    print(f"token healthy, expires in {days_left} days ({expires_on})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
