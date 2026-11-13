<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: A Stale Session Gets the Public Site, Not a 403

As a **Visitor** whose browser still holds a session for a user that no longer exists, I want
every frontend page to log me out and serve me as the public user, instead of a bare 403 on
every page.

## Background

Odoo core builds `request.env` from the raw, unvalidated `session.uid` and runs
`ir.http._match` before it validates the session. website's URL matching reads `website` and
`res.lang` records in that window, so a session whose user had been deleted failed the ACL (access control list) check
there. Seen on hams.com production 2026-09-23/24 as 35 hits for a nonexistent `uid: 99`.

## The Process

1. `ir.http._match` runs the same session check that `_authenticate_explicit` runs later, but
   before URL matching. A dead session is logged out and the request continues as the public
   user (`ir.http._match`, see Entries).

## Verification

`hams_base/tests/test_stale_session_match.py` points a real session at a deleted user and
requests the home page.

## Entries

### `ir.http._match(cls, path)` [@ANCHOR: hams_base_validate_session_before_website_match]

Override of the classmethod `_match` of `ir.http` in `hams_base/models/ir_http.py`. Odoo calls it for every request to
find the routing rule for `path`. `path` is the request path string; it returns what the parent returns (the matched
rule and its arguments) or raises the parent's exceptions such as `NotFound`; it adds none of its own.

Before delegating to `super()._match(path)` it does this: if there is a request and `request.session.uid is not None`, it
calls `security.check_session(request.session, request.env)` (the same check `_authenticate_explicit` makes later). If the
session is invalid, for instance because its user was deleted, it calls `request.session.logout(keep_db=True)` and
replaces `request.env` with an `Environment` for uid `None` (the public user) on the same cursor. The visitor is then
served the public site rather than a 403 from the ACL check on `website`/`res.lang` records. `request` is not passed to
`check_session` because the cursor is read-only here; the later device-log update for valid sessions is unchanged, and
the second check is a cached lookup. No locks, no writes, cheap per request. Security: it only ever downgrades a stale
session to public, never upgrades. Tests: `hams_base/tests/test_stale_session_match.py` (a session pointed at a deleted
user requests the home page).
