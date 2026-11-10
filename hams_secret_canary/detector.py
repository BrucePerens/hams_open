# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Pure string logic of the secret canary: fingerprints, canary generation and the scan. No Odoo, no database."""
import hashlib
import re
import secrets
import string

# A secret or canary is one run of these characters, 16 to 512 long: the shape of an API key, a token or a signing
# secret as it appears delimited by quotes, spaces or punctuation in JSON, HTML, a URL or a log line.
TOKEN_RE = re.compile(r"[A-Za-z0-9_\-+/.]{16,512}")
# Only the first MAX_SCAN_BYTES of a response are scanned, bounding the cost of a response.
MAX_SCAN_BYTES = 1 << 20
SCANNABLE_MIMETYPES = frozenset({"application/json", "application/javascript", "application/xml", "application/xhtml+xml"})
KIND_CANARY = "canary"
KIND_SECRET = "secret"
_ALNUM = string.ascii_letters + string.digits
CANARY_SHAPES = {
    # label: (prefix, alphabet, random length), plausible values for the credentials an attacker looks for first
    "stripe_secret_key": ("sk_live_", _ALNUM, 24),
    "s3_secret_access_key": ("", _ALNUM + "/+", 40),
    "cloudflare_api_token": ("", _ALNUM + "-_", 40),
}


# [@ANCHOR: hams_secret_canary:fingerprint]
def fingerprint(salt, token):
    """Returns the salted SHA-256 hex digest of `token`; the only form in which a secret is ever stored."""
    return hashlib.sha256((salt + token).encode("utf-8")).hexdigest()


# [@ANCHOR: hams_secret_canary:new_salt]
def new_salt():
    """Returns a fresh random per-install salt (32 hex characters)."""
    return secrets.token_hex(16)


# [@ANCHOR: hams_secret_canary:make_canary]
def make_canary(label):
    """Returns a new random decoy value shaped like the credential `label` names (a key of `CANARY_SHAPES`)."""
    prefix, alphabet, length = CANARY_SHAPES[label]
    return prefix + "".join(secrets.choice(alphabet) for _ in range(length))


# [@ANCHOR: hams_secret_canary:is_token]
def is_token(value):
    """True when `value` is exactly one token, the only shape the scan can find."""
    return bool(value) and TOKEN_RE.fullmatch(value) is not None and not value.endswith(".")


# [@ANCHOR: hams_secret_canary:scannable_response]
def scannable_response(response):
    """True for a finished, non-streamed text response (text/*, JSON, JavaScript, XML)."""
    if response.direct_passthrough or not response.is_sequence:
        return False
    mimetype = response.mimetype or ""
    return mimetype.startswith("text/") or mimetype in SCANNABLE_MIMETYPES


# [@ANCHOR: hams_secret_canary:candidates]
def _candidates(token):
    """The token, without leading and trailing dots, and each 16+ character piece between slashes (a secret in a path)."""
    seen = {token}
    yield token
    for candidate in [token.strip("."), *token.split("/")]:
        if len(candidate) >= 16 and candidate not in seen:
            seen.add(candidate)
            yield candidate


# [@ANCHOR: hams_secret_canary:find_matches]
def find_matches(text, salt, table):
    """Returns the `(label, kind)` of every registered fingerprint in `table` ({digest: (label, kind)}) whose token is
    in `text`, each once, in order of first appearance. Cost is one hash per distinct token."""
    found = {}
    for token in {m.group() for m in TOKEN_RE.finditer(text)}:
        for candidate in _candidates(token):
            entry = table.get(fingerprint(salt, candidate))
            if entry is not None:
                found[tuple(entry)] = text.find(token)
    return sorted(found, key=found.get)


# [@ANCHOR: hams_secret_canary:redact]
def redact(text, salt, table):
    """Returns `text` with every token that matches a fingerprint in `table` replaced by `[REDACTED:<label>]`."""
    def replace(match):
        token = match.group()
        for candidate in _candidates(token):
            entry = table.get(fingerprint(salt, candidate))
            if entry is not None:
                return token.replace(candidate, "[REDACTED:%s]" % entry[0])
        return token
    return TOKEN_RE.sub(replace, text)
