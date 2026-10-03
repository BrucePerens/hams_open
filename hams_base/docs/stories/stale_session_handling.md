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
`res.lang` records in that window, so a session whose user had been deleted failed the ACL check
there. Seen on hams.com production 2026-09-23/24 as 35 hits for a nonexistent `uid: 99`.

## The Process

1. `ir.http._match` runs the same session check that `_authenticate_explicit` runs later, but
   before URL matching. A dead session is logged out and the request continues as the public
   user `[@ANCHOR: hams_base_validate_session_before_website_match]`.

## Verification

`hams_base/tests/test_stale_session_match.py` points a real session at a deleted user and
requests the home page.
