<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: One robots.txt Policy for Every Crawler

As an **Administrator**, I want every crawler that fetches third-party pages to honor robots.txt
the same way, so that the rules cannot drift between two hand-written copies.

## The Process

Each crawler fetches robots.txt itself through `urlopen_ssrf_safe()`, keeping its own SSRF label, timeout and size cap, then
hands the outcome to one shared, Odoo-free verdict function. `urlopen_ssrf_safe()` (the fetch helper, which refuses non-public
addresses) and `robots_txt_verdict()` are described in [daemon/README.md](../../daemon/README.md). The module is stdlib only and
has no network access; a fetch that never completes (DNS failure, refused connection, timeout, malformed URL) is handled by the
caller as "allowed", without calling it, because the absence of a robots.txt is not a disallow. Three size caps keep a hostile
file from making matching slow: a line over 4096 characters is skipped, a pattern over 2048 is ignored, a target over 8192 is cut.

### `robots_txt_verdict(url, user_agent, *, body=None, http_status=None)` `[@ANCHOR: zero_sudo:robots_txt_policy_robots_txt_verdict]`

| Argument | Type | Meaning |
|---|---|---|
| `url` | `str` | the page the crawler wants to fetch |
| `user_agent` | `str` | the crawler's User-Agent; its product token picks the rule group |
| `body` | `bytes` or `None` | the raw robots.txt of a successful fetch |
| `http_status` | `int` or `None` | the status code of an HTTP-error response for robots.txt |

Exactly one of `body` and `http_status` must be given, otherwise `ValueError`. Returns `True` when the crawler may fetch the URL.
A body is decoded as UTF-8 with undecodable bytes replaced (a stray byte must not turn a parseable file into an exception) and
judged by `longest_match_verdict`, not `urllib.robotparser`. A status of 401 or 403 disallows everything; any other status, 4xx or
5xx, allows. The 5xx case deliberately differs from the stdlib's `RobotFileParser.read()`, because a transient server error is not
a confirmed disallow: do not "correct" it. Pure, no I/O, no locking.

### `longest_match_verdict(text, url, user_agent)` `[@ANCHOR: zero_sudo:robots_txt_policy_longest_match]`

| Argument | Type | Meaning |
|---|---|---|
| `text` | `str` | the robots.txt text |
| `url` | `str` | the URL to judge |
| `user_agent` | `str` | the crawler's User-Agent |

Returns `True` when allowed. Decided by RFC 9309, not by `urllib.robotparser`, whose rule order differs between Python releases
(older ones take the first matching rule in file order, 3.13 the longest), so the same file was allowed on one interpreter and
blocked on another. The groups naming the product token of `user_agent` (exact, case-insensitive, merged when several do) apply;
with none, the `*` groups replace them. Among their rules the longest matching pattern wins, an Allow wins a tie, and no matching
rule means allowed. An empty pattern is no rule; a pattern starting with neither `/` nor `*`, or longer than 2048 characters,
matches nothing. Pure function; it does not raise for malformed text.

### `_product_token(user_agent)` `[@ANCHOR: zero_sudo:robots_txt_policy_product_token]`

| Argument | Type | Meaning |
|---|---|---|
| `user_agent` | `str` or `None` | a User-Agent header or a `User-agent:` value |

Returns the lower-cased text before the first `/` or whitespace, so `Hamsbot/1.2 (+url)` and `HAMSBOT` agree on `hamsbot`. `None`
or blank gives `""`, which never matches a named group. Pure; used for both the crawler and every `User-agent:` line, so the
comparison is exact and case-insensitive rather than a substring match.

### `_normalise(text)` `[@ANCHOR: zero_sudo:robots_txt_policy_normalise]`

| Argument | Type | Meaning |
|---|---|---|
| `text` | `str` | a path, query or rule pattern |

Returns the percent-escape normal form: non-ASCII and unsafe characters are percent-encoded (the characters a path or query may
carry literally are kept), then each `%XX` that stands for an unreserved character is decoded and the rest are upper-cased, so
`/a%7Eb`, `/a~b` and `/a%7eb` agree. Applied to both the target URL and every pattern before comparing. Pure; no error cases.

### `_target(url)` `[@ANCHOR: zero_sudo:robots_txt_policy_target]`

| Argument | Type | Meaning |
|---|---|---|
| `url` | `str` | the URL to judge; a non-string is treated as empty |

Returns the path plus `?query` that rules are matched against, in normal form and cut at 8192 characters: `/` for an empty
path, with the scheme, host and fragment dropped. An unparsable URL (`ValueError` from `urlsplit`) gives `/`, so a malformed URL
is judged as the site root rather than raising. Pure function.

### `_pattern_matches(pattern, target)` `[@ANCHOR: zero_sudo:robots_txt_policy_pattern_matches]`

| Argument | Type | Meaning |
|---|---|---|
| `pattern` | `str` | a normalised Allow or Disallow pattern |
| `target` | `str` | the normalised path and query |

Returns `True` when the pattern matches the start of the target, with `*` standing for any run of characters and a final `$`
anchoring the end. It is a linear scan that finds each literal piece leftmost, with no regular expression, so a hostile file such
as `/a*a*a*a*b` cannot cause backtracking blow-up. Pure function; the caps on pattern and target size bound the work.

### `_groups(text)` `[@ANCHOR: zero_sudo:robots_txt_policy_groups]`

| Argument | Type | Meaning |
|---|---|---|
| `text` | `str` | the robots.txt text |

Returns a list of `(agents, rules)`, one per group, where `rules` holds `(kind, pattern)` with kind `allow` or `disallow`.
Consecutive `User-agent:` lines share a group; a `User-agent:` line after a rule starts a new one; a rule before any
`User-agent:` is ignored. A leading byte-order mark is stripped, `#` comments are cut, field names are case-insensitive, and a
line over 4096 characters or without `:` is skipped. Unknown fields are ignored. Pure function.

## Verification

`zero_sudo/tests/test_robots_txt_policy.py` covers each body and status-code case.
