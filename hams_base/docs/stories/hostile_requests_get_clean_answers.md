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
   its own canonical link. (Entry `_url_localized` below.)
2. The OpenGraph `og:url` of the page is built by `ir.http._url_for`. A local URL that still holds a dot
   segment after Odoo has tidied it (the double-encoded `%252e%252e` arrives as `%2e%2e`) becomes `/`. A URL
   that is absolute is left alone. (Entry `_url_for` below.)

## XML-RPC faults

Odoo's `/xmlrpc/<service>` and `/xmlrpc/2/<service>` answer an unexpected exception with the whole Python
traceback as the fault string, to anyone, signed in or not. A malformed request body returned library and
source paths and line numbers.

1. An expected error (a user-facing error, an access or missing-record error, an access denial, a
   redirect warning) keeps its message, which is written for the caller.
2. Anything else becomes the generic "Odoo Server Error" fault, in both the `/xmlrpc/2/` form (integer fault
   code) and the legacy `/xmlrpc/` form (string fault code). Odoo still logs the real error in full.
3. In developer mode the stock reply is kept, so a developer still sees the traceback. (Entries `scrubbed_fault_int` and `scrubbed_fault_string` below.)

## Verification

`hams_base/tests/test_canonical_url_dot_segments.py` sends hostile and double-encoded paths with the
dot segments left in the request and requires a 404 with nothing logged at error level.
`hams_base/tests/test_database_lockdown.py` posts a malformed XML-RPC body and requires a reply with no
traceback or path.

## Entries

### `ir.http._url_localized(...)` [@ANCHOR: hams_base:canonical_url_survives_dot_segments]

Classmethod override in `hams_base/models/ir_http.py` of the method that builds the localized, and for the canonical
`<link>` the absolute, URL of a page.

| Argument | Type | Meaning |
|---|---|---|
| `url` | `str` or `None` | Path to localize; the layout passes the raw request path. |
| `lang_code` | `str` or `None` | Language code to put in the URL. |
| `canonical_domain` | `str` or `None` | When set, the absolute canonical form is built with this domain. |
| `prefetch_langs`, `force_default_lang` | `bool` | Passed to the parent unchanged. |

It calls the parent. If Odoo's `tools.urls.urljoin` raises `ValueError` ("Dot segments are not allowed": a `.` or `..`
segment, percent-encoded or not, or bytes that are not UTF-8) and `canonical_domain` is a non-empty string, it returns
`urljoin(canonical_domain, "/")`, the site root; the page is still a 404. Without a `canonical_domain` (the relative form,
which cannot raise this way) the error is re-raised unchanged. Other exceptions are not caught. No side effects, no
locks, one extra join on the error path only. Security: a scanner can no longer produce a 500, an exception ticket
and a traceback with library paths by probing with dot segments. Test:
`tests/test_canonical_url_dot_segments.py`.

### `ir.http._url_for(url_from, lang_code=None)` [@ANCHOR: hams_base:url_for_survives_dot_segments]

Classmethod override for the OpenGraph `og:url`, which `website.seo.metadata._default_website_meta` builds from
`url_join(domain, _url_for(request.httprequest.path))`. `url_from` is the local or absolute URL; `lang_code` as in core.
It returns the parent's answer, except that a string that starts with a single `/` (not `//`) and whose path
still holds a dot segment, judged by `_has_dot_segment` on `urllib.parse.urlsplit(url).path`, becomes `"/"`. That
covers the double-encoded probe `/@fs/%252e%252e/x`, which reaches the server as `%2e%2e`: core resolves a plain
`/a/../b` itself but leaves this form, and the later `url_join` would raise. Absolute URLs (`http://...`, `//host/...`)
and non-strings are returned unchanged. Pure, no database, cannot raise on its own. Fixes exception ticket 74. Test:
`tests/test_canonical_url_dot_segments.py`.

### `_has_dot_segment(path)` [@ANCHOR: hams_base:has_dot_segment]

Module-level function in `hams_base/models/ir_http.py`. `path` is the path part of a URL (`str`). Returns `True` when Odoo's
`tools.urls.urljoin` would refuse it: the path, percent-decoded once with `errors="strict"`, has a `.` or `..` segment
when split on `/`, or the percent-encoded bytes are not valid UTF-8 (`UnicodeDecodeError` is caught and gives `True`).
Decoding happens once only, so a double-encoded `%252e%252e` is not a dot segment here; that is why `_url_for` sees it only
after the request path decoding. Pure; never raises; no database; used by `_url_for`.
Test: `TestCanonicalUrlSurvivesDotSegments.test_05_the_dot_segment_check_matches_odoos_own_refusals`.

### `scrubbed_fault_int(error)` [@ANCHOR: hams_base:xmlrpc_fault_scrub]

In `hams_base/controllers/xmlrpc_lockdown.py`. It builds the XML-RPC fault reply for the `/xmlrpc/2/` endpoints
(integer fault code). `error` is the exception being reported. If the server runs in developer mode, or `error` is an
expected, caller-facing kind (`UserError`, which covers `AccessError` and `MissingError`; `AccessDenied`;
`RedirectWarning`), it returns the core reply `xmlrpc_handle_exception_int(error)`, which keeps the message. Otherwise it
returns the marshalled `xmlrpc.client.Fault(RPC_FAULT_CODE_APPLICATION_ERROR, "Odoo Server Error")`: no traceback, no
exception text, no paths. The real error is still logged in full by Odoo. Returns the XML-RPC response body (the string
`dumps` produces). Pure apart from reading `tools.config["dev_mode"]`. Tests:
`TestXmlrpcFaultsCarryNoTraceback` in `tests/test_database_lockdown.py`.

### `scrubbed_fault_string(error)` [@ANCHOR: hams_base:xmlrpc_fault_scrub_string]

The same for the legacy `/xmlrpc/` endpoint, where the fault code is a string: developer mode or an expected error gives
the core `xmlrpc_handle_exception_string(error)`; anything else gives `dumps(Fault("Odoo Server Error", ""))`. The fault
string carries the generic message in the code position and an empty string, so no traceback or path can leak. Error
classes treated as expected are `EXPECTED_XMLRPC_ERRORS = (UserError, AccessDenied, RedirectWarning)`. Tests:
`test_a_malformed_body_has_no_traceback_or_path_in_the_reply` and `test_a_wrong_password_keeps_its_message` in
`TestXmlrpcFaultsCarryNoTraceback`.

### `xmlrpc_lockdown.install()` [@ANCHOR: hams_base:xmlrpc_lockdown_install]

No arguments, no return value. Replaces two attributes of the stock module (`sys.modules[XMLRPC.__module__]`):
`xmlrpc_handle_exception_int = scrubbed_fault_int` and `xmlrpc_handle_exception_string = scrubbed_fault_string`. It is done
by assignment because the stock `xmlrpc_1` and `xmlrpc_2` handlers look those two functions up by name at call time, and a
subclass of `XMLRPC` did not take over (the stock route still matched first); the dispatchers in `database_lockdown` could
be subclassed because they are registered by routing type. It runs once, when the module is imported (`install()` at the
bottom of the file, imported from `controllers/__init__.py`). Idempotent: running it twice assigns the same functions.
Global effect on the process: every XML-RPC reply. Tests: `test_a_malformed_body_has_no_traceback_or_path_in_the_reply` and
`test_a_good_call_still_works`.

## Lockdown controllers

All of these are in `hams_base/controllers/database_lockdown.py`. `DatabaseManagerLockdown` subclasses core's `Database`
controller; each override keeps the parent's route (type, auth, methods) and begins with `_refuse()`, so outside developer
mode the visitor gets a plain 404 and inside it the core behaviour. This is deliberate: it must hold on every installation
of the open-source module, not only behind our firewall. All the overrides run in the request thread, take no locks and
touch no data when refusing.

### `DatabaseManagerLockdown._refuse()` [@ANCHOR: hams_base:database_lockdown_refuse]

Static method, no arguments, no return value. Raises werkzeug `NotFound` (HTTP 404, the ordinary 404 page) unless
`developer_mode()` is true, i.e. the server was started with `--dev`. Every route override below calls it before
delegating to `super()`, so the core code, which needs only the master password, is never reached on a production server.
It cannot be bypassed by arguments or headers. Tests:
`TestDatabaseManagerIs404.test_pages_and_actions_are_not_found` and `test_developer_mode_still_reaches_the_core_page`.

### `selector(self, **kw)` [@ANCHOR: hams_base:database_lockdown_selector]

Route `/web/database/selector` (`type="http"`, `auth="none"`), the page that lists databases to pick. It calls `_refuse()`,
then returns `super().selector(**kw)`: a 404 on production, the core page in developer mode. `**kw` is the query string,
passed through unchanged. Tests: `test_pages_and_actions_are_not_found` and `test_developer_mode_still_reaches_the_core_page`.

### `manager(self, **kw)` [@ANCHOR: hams_base:database_lockdown_manager]

Route `/web/database/manager`, the database-management page (create, duplicate, drop, back up, restore, change the master
password). Before the lockdown it answered 200 to anyone; it was inert only because `list_db` was off and no `admin_passwd`
was set. Now `_refuse()` then `super().manager(**kw)`. Test: `test_pages_and_actions_are_not_found`.

### `create(self, *args, **kw)` [@ANCHOR: hams_base:database_lockdown_create]

Route `/web/database/create`, POST, `auth="none"`, CSRF off in core. Core takes `master_pwd, name, lang, password` and
creates a database. After `_refuse()` on production nothing in core runs, so no database can be created through this
route. In developer mode the arguments are passed unchanged to the core method. Test: `test_pages_and_actions_are_not_found`.

### `duplicate(self, *args, **kw)` [@ANCHOR: hams_base:database_lockdown_duplicate]

Route `/web/database/duplicate`, POST. Core takes `master_pwd, name, new_name, neutralize_database` and copies a database.
Refused with 404 outside developer mode, so a production database cannot be copied (and with its data downloaded) through
the web. Test: `test_pages_and_actions_are_not_found`.

### `drop(self, *args, **kw)` [@ANCHOR: hams_base:database_lockdown_drop]

Route `/web/database/drop`, POST, `auth="none"`, CSRF off in core. Core takes `master_pwd` and `name` and deletes that
database for anyone who knows the master password. The override takes `*args, **kw` and passes them through unchanged. It
calls `_refuse()` first, so outside developer mode the answer is the plain 404 page and nothing is dropped, whatever the
master password or the database name. In developer mode it returns core's result. No locks, no database access of its own,
no side effects when refusing. Test: `test_pages_and_actions_are_not_found`.

### `backup(self, *args, **kw)` [@ANCHOR: hams_base:database_lockdown_backup]

Route `/web/database/backup`, POST. Core takes `master_pwd, name, backup_format="zip", filestore=True` and streams a full
dump. Refused with 404 outside developer mode, so no backup is served through it; real backups use the backup module's own
worker. Test: `test_pages_and_actions_are_not_found`.

### `restore(self, *args, **kw)` [@ANCHOR: hams_base:database_lockdown_restore]

Route `/web/database/restore`, POST, with no content-length limit in core (an upload). Core takes `master_pwd,
backup_file, name, copy, neutralize_database`. Refused with 404 outside developer mode, before the upload is read into
a database. Test: `test_pages_and_actions_are_not_found`.

### `change_password(self, *args, **kw)` [@ANCHOR: hams_base:database_lockdown_change_password]

Route `/web/database/change_password`, POST, `auth="none"`, CSRF off in core. Core takes `master_pwd` (the current
database-manager password) and `master_pwd_new` and changes the master password in the server configuration. The override
passes its arguments through, calls `_refuse()` first, and so outside developer mode answers the plain 404 page and
changes nothing; in developer mode it returns core's result. The master password is therefore never reachable over HTTP on
a production server. No locks and no side effects when refusing. Test: `test_pages_and_actions_are_not_found`.

### `list(self, *args, **kw)` [@ANCHOR: hams_base:database_lockdown_list]

Route `/web/database/list`, a JSON-RPC route (`type="jsonrpc"`, `auth="none"`) that returns the database names. Outside
developer mode `_refuse()` raises `NotFound`, which the JSON-RPC dispatcher answers with error code 404 and no result (see
`handle_error` below). In developer mode it returns core's list. Test:
`TestDatabaseManagerIs404.test_the_database_list_rpc_is_not_found`.

## Error-body scrubbing

### `HamsJsonRPCDispatcher.handle_error(self, exc)` [@ANCHOR: hams_base:database_lockdown_jsonrpc_handle_error]

Subclass of core's `JsonRPCDispatcher` with `routing_type = "jsonrpc"` (the same type, so it replaces the core class).
`exc` is the exception raised by the endpoint. It returns the JSON-RPC response with `error = {code, message, data}`,
`data` being `scrub_error_body(http.serialize_exception(exc), exc)` (the traceback is dropped, and for an unexpected kind the
class name, message, arguments and context are replaced by the generic text). Code and message: `0` "Odoo Server Error" by
default; `404` "404: Not Found" for `NotFound`; `400` "400: Bad Request" for `BadRequest` (a call that left out an argument
or carried a NUL byte: the caller's mistake, named in `data.message`); `100` "Odoo Session Expired" for
`SessionExpiredException`. In developer mode `data` is untouched. No side effects; Odoo logs the real error elsewhere.
Tests: `test_jsonrpc_handle_error_has_no_traceback` and `test_jsonrpc_handle_error_names_a_bad_request_as_a_400` in
`TestScrubErrorBody`.

### `HamsJson2Dispatcher.handle_error(self, exc)` [@ANCHOR: hams_base:database_lockdown_json2_handle_error]

Subclass of core's `Json2Dispatcher` (`routing_type = "json2"`). If `exc` is an `HTTPException` that already carries a
response, that response is returned as is. Otherwise status and body come from the kind: a `UserError` or
`SessionExpiredException` uses `exc.http_status` and the serialized exception; another `HTTPException` uses its own code
and description (and its `Content-Type` header, asserted to be `text/html; charset=utf-8`); anything else is HTTP 500.
The body then passes through `scrub_error_body`, so no traceback is sent and an unexpected exception shows only "Odoo
Server Error". Returns a JSON response via `request.make_json_response`. The `assert` on the content type would raise
`AssertionError` if core changes that header; to-do: replace it with a safe default. Test:
`TestScrubErrorBody.test_json2_handle_error_has_no_traceback_and_keeps_the_status`.
