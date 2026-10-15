# SPDX-License-Identifier: AGPL-3.0-or-later
"""Stateless, signed challenges for the form shield.

Nothing here touches the database or Redis: a challenge is a string the server signs with
its own key, so it can be issued from any worker and checked by any other, and issuing one
costs a few microseconds. The one piece of state (a challenge may be used once) lives in
`models/form_shield.py`.

Wire format, ASCII only, fields separated by dots (no dot appears inside a field):

    v1.<kind>.<issued>.<expires>.<difficulty>.<nonce>.<ip_tag>.<signature>

`kind` is "pow" (proof of work) or "q" (a plain-text question for visitors whose browser
cannot run the proof of work). The signature covers everything before it. A visitor's answer
is `<challenge>~<solution>`.

The proof of work is SHA-256 over the UTF-8 bytes of `<challenge>:<form_id>:<counter>`, the
counter written in decimal from 0. A solution is the SMALLEST counter whose digest starts with
`difficulty` zero bits. Putting the form id in the hash binds a solution to one form without
the form having to be known when the challenge is issued. static/src/pow/pow_core.js computes
the same hash; both sides test against the vector in tests/test_tokens.py.
"""
import hashlib
import hmac
import ipaddress
import secrets

VERSION = "v1"
KIND_POW = "pow"
KIND_QUESTION = "q"
SIGNATURE_CHARS = 40
RESPONSE_SEPARATOR = "~"
# The names of the fields every shielded HTML form posts (see views/form_shield_templates.xml).
RESPONSE_FIELD = "shield_response"
QUESTION_CHALLENGE_FIELD = "shield_challenge"
QUESTION_ANSWER_FIELD = "shield_answer"
HONEYPOT_FIELD = "fs_homepage"

# Failure reasons (also the `reason` column of the rejection counters).
BAD_FORMAT = "bad_format"
BAD_SIGNATURE = "bad_signature"
WRONG_KIND = "wrong_kind"
EXPIRED = "expired"
IP_CHANGED = "ip_changed"
TOO_FAST = "too_fast"
BAD_SOLUTION = "bad_solution"

NUMBER_WORDS = (
    "zero one two three four five six seven eight nine ten eleven "
    "twelve thirteen fourteen fifteen sixteen seventeen eighteen "
    "nineteen twenty"
).split()


# [@ANCHOR: form_shield:mac]
# Verified by [@ANCHOR: form_shield:test_tampered_challenge_is_refused]
def _mac(key, text):
    digest = hmac.new(key.encode(), text.encode(), hashlib.sha256)
    return digest.hexdigest()


# [@ANCHOR: form_shield:ip_tag]
# Verified by [@ANCHOR: form_shield:test_ip_tag_uses_prefix_and_keeps_no_address]
def ip_prefix(address):
    """The /24 (IPv4) or /48 (IPv6) network of `address`, as text.

    A phone moving between towers keeps its /24 far more often than its address, and a
    household's devices share one, so a challenge fetched on one device survives a change
    of address inside the same network. An unparseable or missing address maps to "none".
    """
    try:
        parsed = ipaddress.ip_address((address or "").strip())
    except ValueError:
        return "none"
    if parsed.version == 4:
        return str(ipaddress.ip_network(f"{parsed}/24", strict=False))
    return str(ipaddress.ip_network(f"{parsed}/48", strict=False))


# [@ANCHOR: form_shield:ip_tag_hash]
# Verified by [@ANCHOR: form_shield:test_ip_tag_uses_prefix_and_keeps_no_address]
def ip_tag(key, address):
    """A keyed hash of the address's network. The address itself is never stored."""
    return _mac(key, "ip|" + ip_prefix(address))[:16]


# [@ANCHOR: form_shield:make_challenge]
# Verified by [@ANCHOR: form_shield:test_tampered_challenge_is_refused]
def make_challenge(key, address, now, difficulty, kind, ttl):
    """A signed challenge issued at `now` (seconds) that stops being valid after `ttl`."""
    fields = [
        VERSION,
        kind,
        str(int(now)),
        str(int(now) + int(ttl)),
        str(int(difficulty)),
        secrets.token_hex(8),
        ip_tag(key, address),
    ]
    body = ".".join(fields)
    return body + "." + _mac(key, body)[:SIGNATURE_CHARS]


class Parsed:
    """The fields of a challenge whose signature has been checked."""

    # [@ANCHOR: form_shield:parsed_challenge]
    # Verified by [@ANCHOR: form_shield:test_tampered_challenge_is_refused]
    def __init__(self, token, kind, issued, expires, difficulty, nonce, tag):
        self.token = token
        self.kind = kind
        self.issued = issued
        self.expires = expires
        self.difficulty = difficulty
        self.nonce = nonce
        self.tag = tag


# [@ANCHOR: form_shield:split_response]
# Verified by [@ANCHOR: form_shield:test_check_refusal_table]
def split_response(response):
    """(challenge, solution) from `<challenge>~<solution>`, or (None, None)."""
    text = (response or "").strip()
    token, separator, solution = text.partition(RESPONSE_SEPARATOR)
    if not separator or not token or len(text) > 400:
        return None, None
    return token, solution.strip()


# [@ANCHOR: form_shield:parse_challenge]
# Verified by [@ANCHOR: form_shield:test_tampered_challenge_is_refused]
def parse_challenge(key, token):
    """(Parsed, None) when the signature holds, else (None, reason)."""
    parts = (token or "").split(".")
    if len(parts) != 8 or parts[0] != VERSION:
        return None, BAD_FORMAT
    body = ".".join(parts[:7])
    expected = _mac(key, body)[:SIGNATURE_CHARS]
    if not hmac.compare_digest(expected, parts[7]):
        return None, BAD_SIGNATURE
    try:
        issued = int(parts[2])
        expires = int(parts[3])
        difficulty = int(parts[4])
    except ValueError:
        return None, BAD_FORMAT
    parsed = Parsed(
        token, parts[1], issued, expires, difficulty, parts[5], parts[6]
    )
    return parsed, None


# [@ANCHOR: form_shield:leading_zero_bits]
# Verified by [@ANCHOR: form_shield:test_pow_vector_matches_the_browser_solver]
def leading_zero_bits(digest):
    """How many zero bits `digest` (bytes) starts with."""
    value = int.from_bytes(digest, "big")
    total = len(digest) * 8
    return total - value.bit_length()


# [@ANCHOR: form_shield:pow_digest]
# Verified by [@ANCHOR: form_shield:test_pow_vector_matches_the_browser_solver]
def pow_digest(token, form_id, counter):
    text = f"{token}:{form_id}:{counter}"
    return hashlib.sha256(text.encode()).digest()


# [@ANCHOR: form_shield:pow_valid]
# Verified by [@ANCHOR: form_shield:test_pow_vector_matches_the_browser_solver]
def pow_valid(token, form_id, counter, difficulty):
    """True when `counter` is a decimal that solves `token` for `form_id`."""
    text = str(counter)
    if not text.isascii() or not text.isdigit() or len(text) > 12:
        return False
    if text != str(int(text)):
        return False
    digest = pow_digest(token, form_id, text)
    return leading_zero_bits(digest) >= difficulty


# [@ANCHOR: form_shield:solve_pow]
# Verified by [@ANCHOR: form_shield:test_pow_vector_matches_the_browser_solver]
def solve_pow(token, form_id, difficulty, start=0):
    """The smallest counter that solves `token`. The browser does this in a Web Worker."""
    counter = start
    while leading_zero_bits(pow_digest(token, form_id, counter)) < difficulty:
        counter += 1
    return counter


# [@ANCHOR: form_shield:question_numbers]
# Verified by [@ANCHOR: form_shield:test_question_fallback_accepts_the_right_answer]
def question_numbers(nonce):
    """The two numbers of the arithmetic question a "q" challenge asks, from its nonce."""
    first = 2 + int(nonce[0:2], 16) % 9
    second = 2 + int(nonce[2:4], 16) % 9
    return first, second


# [@ANCHOR: form_shield:question_text]
# Verified by [@ANCHOR: form_shield:test_question_fallback_accepts_the_right_answer]
def question_text(nonce):
    first, second = question_numbers(nonce)
    return "What is %s plus %s? Type your answer as digits." % (
        NUMBER_WORDS[first],
        NUMBER_WORDS[second],
    )


# [@ANCHOR: form_shield:question_valid]
# Verified by [@ANCHOR: form_shield:test_question_fallback_accepts_the_right_answer]
def question_valid(nonce, answer):
    try:
        first, second = question_numbers(nonce)
    except ValueError:
        return False
    return (answer or "").strip() == str(first + second)
