<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: One robots.txt Policy for Every Crawler

As an **Administrator**, I want every crawler that fetches third-party pages to honor robots.txt
the same way, so that the rules cannot drift between two hand-written copies.

## The Process

1. Each crawler fetches robots.txt itself through `urlopen_ssrf_safe()`, keeping its own SSRF
   label, timeout and size cap, then hands the outcome to one shared, Odoo-free verdict function.
   A successful fetch is parsed as UTF-8 (undecodable bytes replaced); HTTP 401/403 disallows
   everything; any other 4xx allows; a 5xx also allows, deliberately unlike the stdlib's
   `RobotFileParser.read()`, because a transient server error is not a confirmed disallow. A fetch
   that never completes is handled by the caller as "allowed"
   `[@ANCHOR: zero_sudo:robots_txt_policy_robots_txt_verdict]`.

`urlopen_ssrf_safe()` (the fetch helper, which refuses non-public addresses) and the shared verdict
function (`robots_txt_verdict()`) are described in [daemon/README.md](../../daemon/README.md).

## Verification

`zero_sudo/tests/test_robots_txt_policy.py` covers each body and status-code case.
