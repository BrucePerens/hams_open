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

**Status:** Verified by `tests/test_missing_params.py`.
