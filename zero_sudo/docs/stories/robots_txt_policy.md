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
   A successful fetch is parsed as UTF-8 (undecodable bytes replaced) and judged by RFC 9309 longest match
   `[@ANCHOR: zero_sudo:robots_txt_policy_longest_match]` (the longest matching rule wins, an Allow wins a tie,
   a named group replaces the `*` group), not by `urllib.robotparser`, whose rule order differs between Python releases; HTTP 401/403 disallows
   everything; any other 4xx allows; a 5xx also allows, deliberately unlike the stdlib's
   `RobotFileParser.read()`, because a transient server error is not a confirmed disallow. A fetch
   that never completes is handled by the caller as "allowed"
   `[@ANCHOR: zero_sudo:robots_txt_policy_robots_txt_verdict]`.

`urlopen_ssrf_safe()` (the fetch helper, which refuses non-public addresses) and the shared verdict
function (`robots_txt_verdict()`) are described in [daemon/README.md](../../daemon/README.md).

The parser behind that verdict reads a robots.txt the way RFC 9309 does, in small steps, each with its own test:
the product token of a User-Agent is the text before the first `/` or space, lower-cased
`[@ANCHOR: zero_sudo:robots_txt_policy_product_token]`; a path or pattern is put in percent-escape normal form so
`%7E` and `~` agree `[@ANCHOR: zero_sudo:robots_txt_policy_normalise]`; the URL is reduced to the path and query that
rules match `[@ANCHOR: zero_sudo:robots_txt_policy_target]`; a pattern is a prefix in which `*` is any run of
characters and a final `$` anchors the end, matched without a regular expression so a hostile file cannot make it slow
`[@ANCHOR: zero_sudo:robots_txt_policy_pattern_matches]`; and consecutive `User-agent:` lines form one group that
shares the rules after them `[@ANCHOR: zero_sudo:robots_txt_policy_groups]`.

## Verification

`zero_sudo/tests/test_robots_txt_policy.py` covers each body and status-code case.
