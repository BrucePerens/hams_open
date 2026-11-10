# -*- coding: utf-8 -*-
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Operations on a secret that return a result, never the secret.

One copy of each operation, used by `SecretStore` (Tier A, inside the calling process) and by the
broker (Tier B, in its own account), so a module gets the same answer on either tier. Every function
takes the secret as its first argument and returns a signature or a boolean; none returns or logs it.
Plain Python, stdlib only.
"""

import hashlib
import hmac
import time

DIGESTS = {"sha256": hashlib.sha256, "sha512": hashlib.sha512}


# [@ANCHOR: zero_sudo:secret_operations_hmac_sign]
def hmac_sign(secret, payload, digest="sha256"):
    """Hex HMAC of `payload` (str, UTF-8) under `secret`; only sha256 and sha512 are offered."""
    if digest not in DIGESTS:
        raise ValueError(f"unsupported digest {digest!r}")
    if not isinstance(payload, str):
        raise ValueError("payload must be a string")
    return hmac.new(secret.encode("utf-8"), payload.encode("utf-8"), DIGESTS[digest]).hexdigest()


# [@ANCHOR: zero_sudo:secret_operations_hmac_verify]
def hmac_verify(secret, payload, signature, digest="sha256"):
    """True when `signature` (hex) equals the HMAC of `payload`; compared in constant time."""
    if not isinstance(signature, str):
        return False
    return hmac.compare_digest(hmac_sign(secret, payload, digest), signature.lower())


# [@ANCHOR: zero_sudo:secret_operations_stripe_webhook_verify]
def stripe_webhook_verify(secret, payload, signature_header, tolerance=300, now=None):
    """Stripe's webhook check: `signature_header` is `t=<unix time>,v1=<hex>[,v1=...]`; valid when one
    v1 is the HMAC-SHA256 of `"<t>.<payload>"` under the webhook secret and |now - t| <= tolerance.
    A malformed header, no v1, a non-integer t or a non-positive tolerance is simply False."""
    if not isinstance(signature_header, str) or not isinstance(payload, str):
        return False
    if not isinstance(tolerance, int) or tolerance <= 0:
        return False
    timestamp, candidates = None, []
    for item in signature_header.split(","):
        key, _, value = item.strip().partition("=")
        if key == "t":
            timestamp = value
        elif key == "v1":
            candidates.append(value)
    if not timestamp or not timestamp.isdigit() or not candidates:
        return False
    now = time.time() if now is None else now
    if abs(now - int(timestamp)) > tolerance:
        return False
    expected = hmac_sign(secret, f"{timestamp}.{payload}")
    return any(hmac.compare_digest(expected, candidate) for candidate in candidates)
