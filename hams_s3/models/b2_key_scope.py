# SPDX-License-Identifier: AGPL-3.0-or-later
"""Asks Backblaze B2 what an application key may do, so an over-broad key is refused when it is configured."""
import logging

import requests

_logger = logging.getLogger(__name__)
AUTHORIZE_URL = "https://api.backblazeb2.com/b2api/v3/b2_authorize_account"
# Capabilities a bucket-scoped storage key does not need; any one of them lets a leaked key destroy data or widen itself.
BROAD_CAPABILITIES = frozenset({"deleteFiles", "writeBuckets", "deleteBuckets", "writeKeys", "deleteKeys", "bypassGovernance"})


def is_b2_host(host):
    """True when the S3 endpoint host is a Backblaze B2 one (for example s3.us-west-004.backblazeb2.com)."""
    return bool(host) and host.lower().rstrip("/").endswith("backblazeb2.com")


def inspect_b2_key(key_id, key, timeout=10):
    """Returns `(True, [problems])` (an empty list when the key is scoped) or `(False, message)`. Fails closed: an
    answer that is not a clear, readable authorization with a capability list is `(False, ...)`. The problems name the
    capabilities to remove and, when the key is not limited to a bucket, say so."""
    try:
        resp = requests.get(AUTHORIZE_URL, auth=(key_id, key), timeout=timeout)  # audit-ignore-outbound-fetch: fixed Backblaze URL
    except requests.RequestException as error:
        _logger.warning("Backblaze key check failed: %s", type(error).__name__)
        return False, "Backblaze could not be reached to check the key's permissions."
    if resp.status_code != 200:
        return False, "Backblaze did not accept the key (status %s)." % resp.status_code
    try:
        allowed = resp.json()["apiInfo"]["storageApi"]["allowed"]
        capabilities = set(allowed["capabilities"])
    except (ValueError, KeyError, TypeError):
        return False, "Backblaze would not list the key's permissions (the answer was not readable)."
    problems = sorted(capabilities & BROAD_CAPABILITIES)
    if not allowed.get("buckets"):
        problems.append("not limited to one bucket")
    return True, problems
