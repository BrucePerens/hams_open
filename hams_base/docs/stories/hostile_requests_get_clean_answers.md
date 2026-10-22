<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: A Probing Scanner Gets a Clean Answer, Not a Traceback or a Server Error

As the **Operator** of a public site, I want a scanner or a malformed request to get a plain 404 or a
generic fault, instead of a server error, an unhandled-exception ticket, or Python library paths and line
numbers, so that background probing neither pages anyone nor tells an attacker how the server is built.

## Dot segments in a request path

Odoo refuses to join a URL that has a `.` or `..` segment (also percent-encoded, such as `%2e%2e`, or a
path that is not valid UTF-8) by raising `ValueError`. The 404 page and the generic error page build URLs
from the raw request path, so a probe such as `/foo/../x` or `/@fs/%2e%2e/x` turned a plain 404 into a
500, an exception ticket, and a second traceback from the error page itself. Seen on hams.com production
as hundreds of `Dot segments are not allowed` errors a day.

1. The canonical `<link>` in the page layout is built by `ir.http._url_localized`. When Odoo refuses the
   path, the canonical URL becomes the site root. A path like that has no honest canonical address, so the
   page is still a 404 and a crawler is told nothing wrong about any real page. A normal unknown page keeps
   its own canonical link.
   `[@ANCHOR: hams_base:canonical_url_survives_dot_segments]`
2. The OpenGraph `og:url` of the page is built by `ir.http._url_for`. A local URL that still holds a dot
   segment after Odoo has tidied it (the double-encoded `%252e%252e` arrives as `%2e%2e`) becomes `/`. A URL
   that is absolute is left alone.
   `[@ANCHOR: hams_base:url_for_survives_dot_segments]`

## XML-RPC faults

Odoo's `/xmlrpc/<service>` and `/xmlrpc/2/<service>` answer an unexpected exception with the whole Python
traceback as the fault string, to anyone, signed in or not. A malformed request body returned library and
source paths and line numbers.

1. An expected error (a user-facing error, an access or missing-record error, an access denial, a
   redirect warning) keeps its message, which is written for the caller.
2. Anything else becomes the generic "Odoo Server Error" fault, in both the `/xmlrpc/2/` form (integer fault
   code) and the legacy `/xmlrpc/` form (string fault code). Odoo still logs the real error in full.
3. In developer mode the stock reply is kept, so a developer still sees the traceback.
   `[@ANCHOR: hams_base:xmlrpc_fault_scrub]`

## Verification

`hams_base/tests/test_canonical_url_dot_segments.py` sends hostile and double-encoded paths with the
dot segments left in the request and requires a 404 with nothing logged at error level.
`hams_base/tests/test_database_lockdown.py` posts a malformed XML-RPC body and requires a reply with no
traceback or path.
