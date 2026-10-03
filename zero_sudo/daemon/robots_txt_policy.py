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

import urllib.robotparser


# [@ANCHOR: zero_sudo:robots_txt_policy_robots_txt_verdict]
def robots_txt_verdict(url, user_agent, *, body=None, http_status=None):
    """Returns True if `user_agent` may fetch `url`, given the outcome of
    fetching that site's robots.txt: exactly one of `body` (the raw bytes of a
    successful fetch) or `http_status` (the status code of an HTTP-error
    response) must be supplied.

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
    parser = urllib.robotparser.RobotFileParser()
    parser.parse(body.decode("utf-8", errors="replace").splitlines())
    return parser.can_fetch(user_agent, url)
