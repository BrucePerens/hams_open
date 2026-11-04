# Story: Relay Exception And Crash Reports

Bruce, 2026-11-03: "Give the relay a way to send exceptions, signals with stack traces, etc. to hams.com. You can use the
same facility that catches JavaScript errors. Like the JavaScript one, make it record the user and translate that to name
and email if possible."

A `hams_local_relay` that panics, is killed by a fatal signal (SIGSEGV, SIGBUS, SIGILL, SIGFPE, SIGABRT) or hits an error
its code calls unexpected sends a small report to hams.com. It becomes a `server_exception` ticket through the same pipeline as
server and browser errors (`report_event`, `server_exception_tickets.md`), with source "Relay". A ticket from a
panic or an error is titled **"Relay exception: "** plus what happened (`panic in multiplexer::handle_command`); one from a fatal
signal is titled **"Relay crash: "** plus the signal (`SIGSEGV in multiplexer::handle_command`). The two prefixes are the constants
`TITLE_PREFIX_EXCEPTION` and `TITLE_PREFIX_CRASH` in `relay_exception_schema.py`, and the prefix mapping in
`exception_fingerprint.py`; Bruce's dictated "It prefix" was read as "Relay".

## How the relay is authenticated

This module does not authenticate anyone and has no route of its own. The relay logs in to the hams.com bridge on its uplink by
signing the bridge's single-use nonce with its identity key (hams_com, `cloud_link.rs`). The report is then a `relay_call` named
`exception_report` on that connection; the bridge adds its own API key and the **node id that the login proved** and Odoo's
`ham_relay_bridge` route checks both before calling this module. A stranger therefore cannot file a ticket as a relay, because it
cannot log in as one, and cannot name an account: the report has no account, node or user field (an extra key is a rejected
report), and the owner and the operator are found by `ham_relay_bridge` from the node and from the operator's key. The model here
takes the relay id and the two account ids as arguments from that trusted caller only.

## What a report is

### `[@ANCHOR: hams_helpdesk:relay_report_wire]` The report (wire form)

The parameters of the `exception_report` call: one JSON object, at most 16 KiB, UTF-8, no NUL. Exactly the keys below
(`unsymbolised` is optional); any other key, a missing key or a value of the wrong type refuses the whole report. Version 1.

| Key | Type | Meaning, range |
| --- | --- | --- |
| `v` | integer | Schema version, 1. A boolean is refused. |
| `kind` | string | `panic`, `signal` or `error`. |
| `message` | string | The panic or error text, at most 500 characters; scrubbed again here and cut to 300 for storage. |
| `signal` | string | `SIGSEGV`, `SIGBUS`, `SIGILL`, `SIGFPE` or `SIGABRT` when `kind` is `signal`; otherwise `""`. SIGTERM and SIGKILL are a shutdown, not a crash, and are refused. |
| `thread` | string | Name of the thread, at most 64 characters. |
| `frames` | list of strings | 1 to 25 frames, innermost first, each at most 240 characters, no control characters: `crate::path::function (src/file.rs:123)`, `crate::path::function`, or `<unresolved> +0xOFFSET`. |
| `version` | string | Relay version, `0.3.4`; letters, digits and `.+_-`, at most 32. |
| `build` | string | Build identifier, 1 to 64 characters of letters, digits and `._-`. |
| `os` | string | Operating system, 1 to 32 characters. |
| `arch` | string | CPU architecture, 1 to 16 characters. |
| `uptime_s` | integer | Seconds the relay had been running, 0 to 10^9. |
| `count` | integer | Occurrences the relay folded into this report since its last one, 1 to 1,000,000. |
| `occurred_at` | integer | Unix seconds when it happened (after a crash, the time the crash record was written). |
| `log_tail` | list of strings | 0 to 20 log lines, each at most 240 characters, already scrubbed by the relay. |
| `operator_key` | string | The browser key (lower-case hex, 64 to 130 characters; a browser's P-256 key is 130, the `key` of the relay's keying log) of the operator who was acting, or `""`. A key is public information; the bridge looks up the account that registered it. |
| `unsymbolised` | boolean, optional | True when the build has no symbols and the frames are addresses. |

**What is never in a report:** audio, radio traffic, the node id, an account id, environment variables, credentials. The relay
leaves them out and this module scrubs the text again.

**Compatibility:** `v` changes only for an incompatible change; the relay and the server are released separately, so a new
optional key is added to `OPTIONAL_KEYS` here first and sent by the relay later.

### `[@ANCHOR: hams_helpdesk:relay_limits]` The limits

`MAX_BODY_BYTES` 16384, `MAX_FRAMES` 25, `MAX_LOG_LINES` 20, `MAX_MESSAGE_CHARS` 500, `MAX_THREAD_CHARS` 64,
`MAX_FRAME_CHARS` 240, `MAX_LOG_CHARS` 240, `STORED_MESSAGE_CHARS` 300, `MAX_COUNT` 1,000,000, `MAX_UPTIME_SECONDS` 10^9, `MAX_TIME`
(2100-01-01), `KINDS`, `SIGNALS`, `RUNTIME_PREFIXES` (frames that are not blamed). Constants of `relay_exception_schema.py`,
read by the validation functions and the tests; a change here is a change to the wire form above. The rate limits are
`RATE_LIMITS` in `relay_exception.py` (6 a minute and 30 an hour per relay) and the new-ticket cap is
`RELAY_NEW_TICKETS_PER_HOUR` (10) in `exception_fingerprint.py`.

### `[@ANCHOR: hams_helpdesk:relay_report_tuple]` The validated report (`Report`)

A `namedtuple` made by `build_report`; never built from raw input elsewhere.

| Field | Type | Meaning |
| --- | --- | --- |
| `type_name` | str | `relay.<kind>` or `relay.signal.<SIGNAL>`; the fingerprint row's exception type. |
| `module` | str | The relay's module of the blamed frame (`multiplexer`). |
| `frame` | str | The blamed frame, normalised (see `normalize_frame`). |
| `route` | str | Always `relay`. |
| `trace` | str | The text kept as the ticket's sample: kind, message, relay id, version, build, platform, thread, uptime, count, time, frames, scrubbed log tail. |
| `name` | str | The ticket title without its prefix: `panic in <function>`, `error in <function>` or `<SIGNAL> in <function>`. |
| `kind`, `signal` | str | As sent. |
| `count` | int | As sent. |
| `operator_key` | str | As sent; consumed by the bridge before this module is called, kept for tests. |
| `occurred_at` | int | As sent. |
| `key_parts` | tuple | The fingerprint inputs `(type_name, module, frame, route)`. |

### `[@ANCHOR: hams_helpdesk:relay_report_rejected]` `ReportRejected`

Exception raised by the validation functions. `status` is an HTTP-like code (400 invalid, 413 too large) and `reason` a fixed word
(`wrong_keys`, `wrong_kind`, `wrong_signal`, `too_long`, `wrong_frames`, `nul_byte`...), never a piece of the report. Nothing outside
`relay_exception.py` and the tests catches it.

## Functions

### `[@ANCHOR: hams_helpdesk:relay_report_rejected_init]` `ReportRejected(status, reason)`

**Purpose.** Builds the exception. **Call interface.** `status`: int, an HTTP-like code; `reason`: str, a fixed word. Both are kept as attributes; the exception's message is `reason`. No errors, no I/O.

### `[@ANCHOR: hams_helpdesk:relay_is_int]` `_is_int(value)`

**Purpose.** True for an integer that is not a boolean (Python counts `True` as an int; a report may not). **Call interface.** `value`: any object. Returns bool. Pure.

### `[@ANCHOR: hams_helpdesk:relay_clean]` `_clean(text)`

**Purpose.** Removes control characters from free text before it is stored or shown. **Call interface.** `text`: str. Returns it with every control character except tab and newline replaced by a space (ESC sequences, NUL, bell). Pure.

### `[@ANCHOR: hams_helpdesk:relay_path_of]` `_path_of(normalized)`

**Purpose.** The path to compare a frame by: a qualified path `<crate::Type as Trait>::f` loses its leading `<` so it matches the crate prefix; `<unresolved>` is kept as it is. **Call interface.** `normalized`: str from `normalize_frame`. Returns str. Pure.

### `[@ANCHOR: hams_helpdesk:relay_function_label]` `_function_label(path)`

**Purpose.** The function as a ticket title shows it: the path without the leading `hams_local_relay::`. **Call interface.** `path`: str, a normalised frame. Returns str (the path unchanged when it is another crate's). Pure.

### `[@ANCHOR: hams_helpdesk:relay_parse_body]` `parse_body(raw)`

**Purpose.** Validates a report that arrives as bytes (the size check, JSON decoding, the NUL check), then `validate_report`.
**Call interface.** `raw`: `bytes`, borrowed. Returns the parsed `dict`. Raises `ReportRejected` 413 `too_large`, 400 `nul_byte`,
400 `not_json`, or anything `validate_report` raises. **Special concerns.** Pure, no I/O, any thread. Not used by the bridge route
(whose parameters are already parsed) but kept for a direct route and the tests. **See also.** `validate_report`.

### `[@ANCHOR: hams_helpdesk:relay_validate_report]` `validate_report(data)`

**Purpose.** The single definition of a valid report. **Call interface.** `data`: any object, borrowed, not changed. Returns `data`.
Raises `ReportRejected`: 400 `not_an_object`, `wrong_keys` (missing or extra key), `wrong_version`, `wrong_type`, `wrong_kind`,
`wrong_signal`, `too_long`, `wrong_version`/`wrong_build`/`wrong_os`/`wrong_arch`, `wrong_operator_key`, `wrong_uptime`,
`wrong_count`, `wrong_time`, `wrong_frames`, `wrong_log_tail`; 413 `too_large` when the JSON encoding exceeds 16 KiB. **Special
concerns.** Pure and cheap; the first failure wins. Booleans are not accepted as integers.

### `[@ANCHOR: hams_helpdesk:relay_scrub_line]` `scrub_line(text, limit)`

**Purpose.** Makes one free-text value storable. **Call interface.** `text`: str; `limit`: int, the longest result. Returns the text
with control characters replaced by a space, newlines flattened, then `exception_scrub.scrub_text` (strict: emails, URLs, IP
addresses, `password=`-style pairs, bearer tokens, long tokens, hex strings, callsigns, long digit runs), cut to `limit`. No errors.
**Special concerns.** Defence in depth: the relay scrubs first and this runs on whatever arrives. A callsign in a log line is
replaced, which is intended; the owner is linked to the ticket instead.

### `[@ANCHOR: hams_helpdesk:relay_normalize_frame]` `normalize_frame(frame)`

**Purpose.** Makes the same function in two builds the same text, so the fingerprint is stable. **Call interface.** `frame`: str.
Returns the function path with the `(file:line)` suffix, `+0x` offsets and addresses, the `::h<16 hex>` hash, closure numbers
(`{closure#3}` and `{{closure}}` become `{closure}`) and generic arguments after a name (`Vec<T,A>` becomes `Vec<>`) removed; an
empty result is `<unresolved>`. **Special concerns.** Pure. Example: `hams_local_relay::mux::run::h0123456789abcdef (src/mux.rs:12)`
becomes `hams_local_relay::mux::run`.

### `[@ANCHOR: hams_helpdesk:relay_top_frame]` `top_frame(frames)`

**Purpose.** Chooses the frame a crash is blamed on. **Call interface.** `frames`: non-empty list of str. Returns
`(normalised path, module)`: the first frame in `hams_local_relay::` (not the crash reporter's own), else the first that is not the
runtime's (`RUNTIME_PREFIXES`), else the first. `module` is the path component after the crate name, or the first component of
another crate's path. **Special concerns.** Pure. A stripped (unsymbolised) build blames `<unresolved>`: all such crashes share one
ticket per kind, which is the honest result.

### `[@ANCHOR: hams_helpdesk:relay_build_report]` `build_report(data, relay_id="unknown", now=None)`

**Purpose.** Turns a validated dict into the `Report`. **Call interface.** `data`: dict (validated again); `relay_id`: str from the
authenticated link, reduced to letters, digits and `_.:-` and cut to 80 characters (it is a hash of the relay's key, not scrubbed as a token), put into the trace only; `now`: unix seconds for tests (a report dated more than a day ahead is
shown at that bound). Returns `Report`. Raises `ReportRejected`. **Special concerns.** Pure. The fingerprint inputs contain the
kind (and signal), module and normalised frame only, never the message, build, line number, thread or count.

### `[@ANCHOR: hams_helpdesk:relay_client_key]` `_client_key(relay_id)`

**Purpose.** The Redis key part for one relay. **Call interface.** `relay_id`: str. Returns `hams_helpdesk:relay_exception:<24 hex>`,
a hash of the database name and the relay id; the id is not in the key. **Special concerns.** No I/O.

### `[@ANCHOR: hams_helpdesk:relay_rate_limited]` `_rate_limited(relay_id)`

**Purpose.** Holds a relay in a panic loop to `RATE_LIMITS`. **Call interface.** `relay_id`: str. Returns True when over a limit, and
also when Redis cannot be reached (fail closed: a report that cannot be metered is dropped). Each call counts one use, in both
windows. **Special concerns.** Redis I/O; the counters expire on their own; a failure is logged at info.

### `[@ANCHOR: hams_helpdesk:relay_record]` `_record(report, owner_uid=None, operator_uid=None)`

**Purpose.** Hands a `Report` to `report_event` with source `relay`, reporter label "relay exception report" and `weight` the relay's
`count`. **Call interface.** `report`: `Report`; `owner_uid`, `operator_uid`: res.users ids or None (anything else is ignored). The owner
is the primary linked account, the operator an extra one; with only an operator, it is the primary. Returns True when it wrote, False
when the once-a-minute gate or a cap held it back. **Special concerns.** Writes on the reporter's own cursor as the service
account; the pipeline links only real, active, non-service accounts. May raise only if `report_event` does, which it does not.

### `[@ANCHOR: hams_helpdesk:relay_submit]` `_submit(params, relay_id, owner_uid=None, operator_uid=None)`

**Purpose.** The whole path for one report, called by the bridge's `exception_report` handler. **Call interface.** `params`: the report
dict; `relay_id`: the id from the login; the two account ids as above. Returns `"recorded"` (written, or held back by the gate or a
cap, which a sender cannot tell apart), `"limited"` (rate limit or limiter down) or `"rejected"` (invalid, or the recorder failed).
Never raises; a failure is logged on the reporter's own logger and never reported again. **Special concerns.** Order: rate limit,
then validation, so an invalid flood is also metered.

## Who is linked, and what staff see

`MEMBER_SOURCES` now includes `relay`. The relay's owner (and, when known, the operator who was acting) are linked to the fingerprint
as Reporting members, exactly as a signed-in browser visitor is (section 17 of `server_exception_tickets.md`): staff see name,
callsign(s) and email computed live from the account, the guardian instead of a minor, nothing for a closed account. A relay with no
linked account is anonymous; the relay id is in the trace. `report_event` has two new optional arguments: `extra_uids` (more accounts
affected by the same report, linked the same way and counted by the once-a-minute gate so a new member is listed at once) and
`weight` (how many occurrences the report stands for). The ticket text says what is collected and that no audio and no radio
traffic is, and makes no promise about privacy.

## Deploy

Needs `odoo -u hams_helpdesk` (a new selection value on the fingerprint row and a new abstract model), then an Odoo restart so the
workers load the module code.

*Verified by `hams_helpdesk/tests/test_relay_exceptions.py`.*
