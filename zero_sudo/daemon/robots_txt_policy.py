# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later

"""
Pure-stdlib, Odoo-free robots.txt verdict policy shared by every crawler that
fetches third-party pages through `ssrf_safe_fetch.urlopen_ssrf_safe()`.

Why this exists: two independent crawlers (an Odoo model and a standalone
daemon) each fetched robots.txt through `urlopen_ssrf_safe()` -- which is why
`RobotFileParser.read()` can't be used directly: it fetches with a plain,
unguarded `urlopen()` -- and each re-implemented, by hand, the same mapping from
"what the robots.txt fetch returned" to "may this URL be fetched". Two copies of
a policy drift; this module is the one copy.

Deliberately contains NO network access. Each caller keeps doing its own
robots.txt fetch through `urlopen_ssrf_safe()` (so each keeps its own SSRF
context label, timeout, size cap, and its own SSRF-rejection semantics -- one
caller refuses the whole URL, the other lets the real page fetch enforce it),
then hands the outcome here:

- the fetch succeeded: pass the raw `body` bytes;
- the server answered with an HTTP error: pass its `http_status`;
- the fetch could not complete at all (DNS failure, connection refused, a
  timeout, a malformed URL): the caller returns True itself, without calling
  this -- absence of a robots.txt is not a disallow.

MUST NOT import anything from `odoo` (same "CRITICAL DAEMON DECOUPLING" rule as
the rest of `zero_sudo/daemon/`): the standalone daemon caller reaches this
module through a plain `sys.path` hop with no `odoo` package available.
"""

import re
from urllib.parse import quote, urlsplit

# Size caps, so a hostile robots.txt cannot make the match slow (the same limits as hams_com's crawlers).
MAX_LINE_CHARS = 4096
MAX_PATTERN_CHARS = 2048
MAX_TARGET_CHARS = 8192

_UNRESERVED = frozenset("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~")
_PERCENT_ESCAPE = re.compile(r"%([0-9A-Fa-f]{2})")
# Characters left as written when non-ASCII text is percent-encoded: everything a path or query may carry literally.
_LITERAL_SAFE = "/%:@!$&'()*+,;=~-._?"


def _product_token(user_agent):
    """The lower-cased product token of a User-Agent or `User-agent:` value: the text before the first "/" or space."""
    text = (user_agent or "").strip()
    return re.split(r"[/\s]", text, maxsplit=1)[0].strip().lower() if text else ""


def _normalise(text):
    """Percent-escape normal form, so "/a%7Eb", "/a~b" and "/a%7eb" agree."""

    def fix(match):
        char = chr(int(match.group(1), 16))
        return char if char in _UNRESERVED else "%" + match.group(1).upper()

    return _PERCENT_ESCAPE.sub(fix, quote(text, safe=_LITERAL_SAFE))


def _target(url):
    """The path plus "?query" the rules are matched against ("/" for an empty path; the fragment is dropped)."""
    try:
        parts = urlsplit((url if isinstance(url, str) else "").strip())
    except ValueError:
        return "/"
    return _normalise((parts.path or "/") + ("?" + parts.query if parts.query else ""))[:MAX_TARGET_CHARS]


def _pattern_matches(pattern, target):
    """RFC 9309 path match: a prefix match in which `*` is any run of characters and a final `$` anchors the end.
    A linear scan with leftmost matching of each literal piece (no regular expression, so no backtracking blow-up)."""
    anchored = pattern.endswith("$")
    body = pattern[:-1] if anchored else pattern
    pieces = body.split("*")
    if len(pieces) == 1:
        return target == body if anchored else target.startswith(body)
    if not target.startswith(pieces[0]):
        return False
    position = len(pieces[0])
    for piece in pieces[1:-1]:
        found = target.find(piece, position)
        if found < 0:
            return False
        position = found + len(piece)
    tail = pieces[-1]
    if anchored:
        return len(target) - len(tail) >= position and target.endswith(tail)
    return target.find(tail, position) >= 0


def _groups(text):
    """[(agents, rules)] for every user-agent group: consecutive `User-agent:` lines form one group, a `User-agent:`
    line after a rule starts a new one, a rule before any `User-agent:` is ignored. `rules` holds (kind, pattern)."""
    groups, agents, rules, seen_rule = [], [], [], False
    for raw in text.lstrip("\ufeff").splitlines():
        if len(raw) > MAX_LINE_CHARS:
            continue
        line = raw.split("#", 1)[0].strip()
        if ":" not in line:
            continue
        field, _sep, value = line.partition(":")
        field, value = field.strip().lower(), value.strip()
        if field == "user-agent":
            if seen_rule and agents:
                groups.append((agents, rules))
                agents, rules, seen_rule = [], [], False
            if value:
                agents.append(value)
        elif field in ("allow", "disallow") and agents:
            rules.append((field, value))
            seen_rule = True
    if agents:
        groups.append((agents, rules))
    return groups


# [@ANCHOR: zero_sudo:robots_txt_policy_longest_match]
def longest_match_verdict(text, url, user_agent):
    """May `user_agent` fetch `url` under the robots.txt `text`? RFC 9309, decided here and not by
    `urllib.robotparser`, whose rule order differs between Python releases (the older ones take the FIRST matching rule in
    file order, 3.13 takes the longest), so the same file was allowed on one interpreter and blocked on another.

    The group(s) naming the product token of `user_agent` (exact, case-insensitive, merged when several do) apply; with
    none, the `*` group(s). Among their rules the longest matching pattern wins, an Allow wins a tie, and no matching
    rule means allowed. An empty pattern is no rule; a pattern starting with neither `/` nor `*` matches nothing."""
    wanted = _product_token(user_agent)
    groups = _groups(text)
    chosen = [rules for agents, rules in groups if wanted and any(_product_token(a) == wanted for a in agents)]
    if not chosen:
        chosen = [rules for agents, rules in groups if any(a == "*" for a in agents)]
    target = _target(url)
    best = None  # (pattern length, kind == "allow")
    for rules in chosen:
        for kind, pattern in rules:
            normal = _normalise(pattern)
            if not normal or len(normal) > MAX_PATTERN_CHARS or normal[0] not in "/*":
                continue
            if _pattern_matches(normal, target):
                key = (len(normal), kind == "allow")
                if best is None or key > best:
                    best = key
    return best is None or best[1]


# [@ANCHOR: zero_sudo:robots_txt_policy_robots_txt_verdict]
def robots_txt_verdict(url, user_agent, *, body=None, http_status=None):
    """Returns True if `user_agent` may fetch `url`, given the outcome of
    fetching that site's robots.txt: exactly one of `body` (the raw bytes of a
    successful fetch) or `http_status` (the status code of an HTTP-error
    response) must be supplied.

    Body: judged by `longest_match_verdict()`, not `urllib.robotparser`.

    Status-code policy, matching `RobotFileParser.read()` where it has one:
    - 401/403: disallow everything (the site is refusing robots outright);
    - any other 4xx: allow everything (no robots.txt published);
    - 5xx (or any other non-2xx status): allow. This deliberately DIFFERS from
      `RobotFileParser.read()`, which leaves neither flag set for a 5xx and so
      makes `can_fetch()` return False -- a transient server error fetching
      robots.txt is not grounds to block the real fetch on an unconfirmed
      disallow. Do not "correct" this to the stdlib's behavior.

    The body is decoded as UTF-8 with `errors="replace"`: robots.txt is
    specified as UTF-8, and a stray undecodable byte must not turn a parseable
    file into an exception."""
    if (body is None) == (http_status is None):
        raise ValueError("robots_txt_verdict() needs exactly one of body or http_status")
    if http_status is not None:
        return http_status not in (401, 403)
    return longest_match_verdict(body.decode("utf-8", errors="replace"), url, user_agent)
