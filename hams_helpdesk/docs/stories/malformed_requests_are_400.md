# Story: A Malformed Request Gets a 400, Not a 500 and a Ticket

As an **administrator** reading the Server Exceptions list, I want a scanner's or a stale link's
malformed request to be answered with a plain 400 and no server-exception ticket, so that real faults are
not buried under requests whose only mistake was the sender's. (The automatic ticket pipeline is in
`server_exception_tickets.md`; a real handler bug must stay visible, so each of these is logged at warning.)

Every route goes through `ir.http._dispatch`, which `hams_helpdesk` wraps.
`[@ANCHOR: hams_helpdesk:missing_required_params_is_400]`

1. **A NUL byte.** PostgreSQL cannot store or compare a string with a NUL (`%00`), so a handler that
   searched with one raised `ValueError` and answered 500. A NUL byte in the request path, or in any text
   parameter at any depth of a query, form or JSON body, is refused with a 400
   `[@ANCHOR: hams_helpdesk:request_has_nul_byte]`. A NUL in the path is refused even earlier, while the
   URL is being matched, before any converter looks anything up (the member-site converters search the
   database during matching, and the page lookup that follows a 404 would too), and is a 400, not a 404, so
   that fall-back is never tried `[@ANCHOR: hams_helpdesk:nul_byte_in_path]`.
2. **A missing required parameter.** A route called without a parameter its handler cannot do without (a
   stock handler declares them as required positionals, such as an `app_id` or `tx_id`) raised `TypeError`
   and was a 500. The reply is now a 400 that names the missing parameters (never a value). This covers
   page routes and JSON-RPC routes.
3. **Text where a number belongs.** A handler that turns a parameter into a number and gets text
   (`int("abc")`) is answered with a 400 "A request parameter is not a number".
4. **Text where a date belongs.** Stock `/blog` passes `?date_begin=` and `?date_end=` straight into a
   search domain, and the ORM raised `ValueError` while optimizing it (45 requests from one scanner, 500 each,
   and two tickets). Such a request is answered with a 400 "A request parameter is not a date"
   `[@ANCHOR: hams_helpdesk:bad_date_is_400]`.
5. **How the date case is told from a real bug.** `_error_is_unreadable_date` decides by where the error
   was raised, not by its message, because the messages vary ("Invalid term 'x' in expression date",
   "Invalid isoformat string", "month must be in 1..12", "day is out of range"). The error is the request's
   own unreadable date when it passes through Odoo's date parser (`parse_date`, `parse_iso_date`), or through the
   ORM's date converter with the text of one of the request's parameters in its message (or Python's
   impossible month, day or year message). A date the server built wrongly itself is not matched, so that
   bug still surfaces as a 500 and a ticket `[@ANCHOR: hams_helpdesk:error_is_unreadable_date]`.

6. **Any other malformed input on a public page is a 404.** On a public page route (type `http`, no login, not a
   CSRF-off POST endpoint such as a webhook) an exception that the caller's own input caused, and that none of the
   400s above matched, is answered 404 and logged at INFO with the route and class only (Bruce, 2026-10-19: "things
   will be probing our site continually") `[@ANCHOR: hams_helpdesk:malformed_input_is_404]`. Which routes count is
   decided from the route's own declaration `[@ANCHOR: hams_helpdesk:input_error_route_scope]`. Only positive evidence
   converts `[@ANCHOR: hams_helpdesk:classify_request_input_error]`: the database refused a value, a lookup found no
   record, the message quotes a value sent (three letters or more)
   `[@ANCHOR: hams_helpdesk:input_error_message_quotes_a_value]`, or the handler failed on the very object sent, found
   by identity among the locals of the innermost frames `[@ANCHOR: hams_helpdesk:input_error_callers_object_is_a_local]`
   `[@ANCHOR: hams_helpdesk:input_error_inner_frames]`. "Sent" means the text, number and list leaves of the request's
   parameters, never a record a URL converter looked up `[@ANCHOR: hams_helpdesk:request_input_values]`. Every other
   exception stays a 500 and a ticket.
7. **Odoo's own parsing of a parameter.** Stock `/web/content` and `/web/image` read `?download=` with
   `str2bool`, which raises `ValueError("Use 0/1/yes/no/true/false/on/off")` for anything but its words; a scanner's
   `/web/content/1297/<file>.pdf?download=514750499` on perens.com was a 500 and ticket 103 (2026-10-25). Stock
   `/web/assets/...` asserts that `?assets_params=` is a dict. Either is the caller's, and a 404, when the error was
   raised in Odoo's `str2bool` called from a controller on a value the request carried (compared after lower-casing,
   so `?download=` and `?download=x` count too), or by an `assert` in one of Odoo's stock controllers on a parameter
   as sent, or on a text built from one (`assert not '%' in url`, url made from the caller's filename): the names
   the `assert` statement reads are taken from its source line
   `[@ANCHOR: hams_helpdesk:input_error_assert_names]`, and one of them must hold the very object the caller sent or a
   text containing a sent value of three characters or more.
   `str2bool` on a setting, or an `assert` in our own code or on a value the handler built with nothing sent in it, stays a 500
   `[@ANCHOR: hams_helpdesk:input_error_odoo_parameter_parsing]`. A non-numeric `?width=` or `?height=` on
   `/web/image` is the not-a-number 400 of item 3.
8. **An error page that cannot be built for the address.** The website layout builds `<meta og:url>` from the request
   path with `urljoin`, which refuses a path with dot segments (`/.//.env`) or an absolute URL in it, so the 404 page
   for a scanner's path failed to render and the visitor got a 500. When the original answer was a 4xx, werkzeug's
   own bare page for that status is sent instead `[@ANCHOR: hams_helpdesk:render_error_is_the_urls]`.

**Status:** Verified by `tests/test_missing_params.py` and `tests/test_request_input_errors.py` (items 6 and 7, with
`tests/test_public_route_fuzz.py` sending scanner garbage to every public GET route).
