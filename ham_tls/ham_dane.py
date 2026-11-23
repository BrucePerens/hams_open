# SPDX-License-Identifier: AGPL-3.0-or-later
"""DANE-EE check for .ham servers: the TLSA records come from the signed .ham zone (resolver is the caller's)."""

import hashlib

_DANE_EE, _FULL_CERT = 3, 0
_MATCHING = {0: lambda data: data, 1: lambda data: hashlib.sha256(data).digest(), 2: lambda data: hashlib.sha512(data).digest()}


# [@ANCHOR: ham_dane:tlsa_matches]
def tlsa_matches(cert_der, records):
    """True if any (usage, selector, matching_type, data) TLSA record pins this certificate. Only DANE-EE (3) on
    the full certificate (selector 0) is supported; no records, or only other kinds, means refuse."""
    for usage, selector, matching, data in records:
        if usage == _DANE_EE and selector == _FULL_CERT and matching in _MATCHING and _MATCHING[matching](cert_der) == data:
            return True
    return False
