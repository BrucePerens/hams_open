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

## Lockdown controllers: interface

- `[@ANCHOR: hams_base:database_lockdown_refuse]` `DatabaseManagerLockdown._refuse()` raises `NotFound` (a plain 404) unless the server runs in developer mode; every database-manager override calls it first. **Tests:** `TestDatabaseManagerIs404.test_pages_and_actions_are_not_found`, `TestDatabaseManagerIs404.test_developer_mode_still_reaches_the_core_page`.
- `[@ANCHOR: hams_base:database_lockdown_selector]` `/web/database/selector` answers a plain 404 outside developer mode and the core page in developer mode. **Tests:** `TestDatabaseManagerIs404.test_pages_and_actions_are_not_found`, `TestDatabaseManagerIs404.test_developer_mode_still_reaches_the_core_page`.
- `[@ANCHOR: hams_base:database_lockdown_manager]` `/web/database/manager` answers a plain 404 outside developer mode and the core page in developer mode. **Tests:** `TestDatabaseManagerIs404.test_pages_and_actions_are_not_found`.
- `[@ANCHOR: hams_base:database_lockdown_create]` `/web/database/create` answers a plain 404 outside developer mode, so no database is created through it. **Tests:** `TestDatabaseManagerIs404.test_pages_and_actions_are_not_found`.
- `[@ANCHOR: hams_base:database_lockdown_duplicate]` `/web/database/duplicate` answers a plain 404 outside developer mode. **Tests:** `TestDatabaseManagerIs404.test_pages_and_actions_are_not_found`.
- `[@ANCHOR: hams_base:database_lockdown_drop]` `/web/database/drop` answers a plain 404 outside developer mode, so no database is dropped through it. **Tests:** `TestDatabaseManagerIs404.test_pages_and_actions_are_not_found`.
- `[@ANCHOR: hams_base:database_lockdown_backup]` `/web/database/backup` answers a plain 404 outside developer mode, so no backup is served through it. **Tests:** `TestDatabaseManagerIs404.test_pages_and_actions_are_not_found`.
- `[@ANCHOR: hams_base:database_lockdown_restore]` `/web/database/restore` answers a plain 404 outside developer mode. **Tests:** `TestDatabaseManagerIs404.test_pages_and_actions_are_not_found`.
- `[@ANCHOR: hams_base:database_lockdown_change_password]` `/web/database/change_password` answers a plain 404 outside developer mode. **Tests:** `TestDatabaseManagerIs404.test_pages_and_actions_are_not_found`.
- `[@ANCHOR: hams_base:database_lockdown_list]` `/web/database/list` (a JSON-RPC call) answers error code 404 with no result outside developer mode. **Tests:** `TestDatabaseManagerIs404.test_the_database_list_rpc_is_not_found`.
- `[@ANCHOR: hams_base:database_lockdown_jsonrpc_handle_error]` `HamsJsonRPCDispatcher.handle_error(exc)` builds the JSON-RPC error reply: a 404 for `NotFound`, a 400 naming the caller's mistake for `BadRequest`, code 100 for an expired session, otherwise the generic message, and the body never carries a traceback or an unexpected exception's text. **Tests:** `TestScrubErrorBody.test_jsonrpc_handle_error_has_no_traceback`, `TestScrubErrorBody.test_jsonrpc_handle_error_names_a_bad_request_as_a_400`.
- `[@ANCHOR: hams_base:database_lockdown_json2_handle_error]` `HamsJson2Dispatcher.handle_error(exc)` builds the JSON-2 error reply with the exception's own HTTP status and a scrubbed body: user errors and HTTP errors keep their message, anything else is the generic 500 message, and no traceback is sent. **Tests:** `TestScrubErrorBody.test_json2_handle_error_has_no_traceback_and_keeps_the_status`.
- `[@ANCHOR: hams_base:xmlrpc_fault_scrub_string]` `scrubbed_fault_string(error)` is the legacy `/xmlrpc/` fault reply (string fault code): the core's own reply for an expected error or in developer mode, otherwise a generic `Odoo Server Error` fault with no traceback or path. **Tests:** `TestXmlrpcFaultsCarryNoTraceback.test_a_malformed_body_has_no_traceback_or_path_in_the_reply`, `TestXmlrpcFaultsCarryNoTraceback.test_a_wrong_password_keeps_its_message`.
- `[@ANCHOR: hams_base:xmlrpc_lockdown_install]` `install()` replaces the stock XML-RPC module's two fault builders with the scrubbing ones where the stock methods look them up; it runs once at import. **Tests:** `TestXmlrpcFaultsCarryNoTraceback.test_a_malformed_body_has_no_traceback_or_path_in_the_reply`, `TestXmlrpcFaultsCarryNoTraceback.test_a_good_call_still_works`.

## Dot-segment check: interface

- `[@ANCHOR: hams_base:has_dot_segment]` `_has_dot_segment(path)` takes the path part of a URL (a `str`) and returns True when Odoo's `tools.urls.urljoin` would refuse it: a `.` or `..` segment, also percent-encoded (`%2e%2e`, `..%2f`), or percent-encoded bytes that are not valid UTF-8. It decodes once only, so a double-encoded `%252e%252e` is not a dot segment. Pure, no database, never raises. Used by `ir.http._url_for`. **Tests:** `TestCanonicalUrlSurvivesDotSegments.test_05_the_dot_segment_check_matches_odoos_own_refusals`.
