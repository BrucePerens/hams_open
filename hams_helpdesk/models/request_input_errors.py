# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Which exceptions out of a public page route were caused by what the caller sent, not by a server fault.

Scanners probe every public route with garbage (text where a date belongs, 25-digit ids, NUL bytes, ``a[]=1``
lists, SQL fragments). A route that does not check its input raises ValueError, KeyError, TypeError or a database
DataError; Odoo answers 500 and a server-exception ticket is filed per probe. ``ir.http._dispatch`` (see
ir_http_exception.py) turns the exceptions this module recognises into a 404.

Nothing here touches the database or the request object, so it is unit-testable with plain exceptions. The rule is
deliberately narrow: an exception is the caller's only when there is positive evidence of it, and every other
exception stays a 500 so a genuine server bug is still seen and still files a ticket. The evidence is one of

* the database refused a value it was given (``psycopg2.DataError``: bad number text, out-of-range integer, bad
  timestamp text, over-long string) and the request carried parameters;
* ``ensure_one()`` found no record ("Expected singleton: res.users()") and the request carried parameters: a lookup by a
  token or id that matches nothing;
* the text of one of the request's own values appears in the exception message (ORM "Invalid term 'ijyM'",
  ``KeyError: 'zz'``, ``invalid literal for int() with base 10: 'abc'``);
* the very object the caller sent (identity, not equality) is a local variable of one of the innermost frames: a
  handler that called ``.split()`` or ``.get()`` on a parameter that arrived as the wrong type.

A TypeError or AttributeError is accepted only on the last of these, because their messages never quote a value and
a bare TypeError is far more often a code bug than a caller's mistake.
"""

import re
import traceback

import psycopg2

# The only classes ever converted. Everything else (UserError, AccessError, NameError, RuntimeError, QWebError,
# OperationalError, IntegrityError, ...) is left alone.
INPUT_ERROR_TYPES = (
    ValueError,  # includes UnicodeError and json.JSONDecodeError
    KeyError,
    IndexError,
    TypeError,
    AttributeError,
    OverflowError,
    psycopg2.DataError,
)

# A value shorter than this is not looked for inside a message: "a" is in every message.
MIN_QUOTED_VALUE_LENGTH = 3
# How many of the innermost frames are searched for the caller's own object. The handler or its parsing helper is
# at the bottom of the traceback; frames above are Odoo's dispatch machinery, whose locals hold the whole request.
INNER_FRAMES = 3
# What ensure_one() says of an empty recordset: "Expected singleton: res.users()". A recordset of two or more prints
# its ids ("res.users(1, 2)"), which is a different, real, bug and does not match.
_EMPTY_RECORDSET_MESSAGE = re.compile(r"Expected singleton: [\w.]+\(\)")


def request_values(params):
    """The text/number leaves of a parameter dict, at any depth: the things the caller chose.

    Records and other objects that Odoo's URL converters put into ``params`` are not the caller's text and are left
    out, so the identity check below cannot match a server-side object."""
    leaves = []
    stack = [params]
    while stack:
        item = stack.pop()
        if isinstance(item, dict):
            stack.extend(item.values())
        elif isinstance(item, (list, tuple, set, frozenset)):
            leaves.append(item)
            stack.extend(item)
        elif isinstance(item, (str, bytes)):
            leaves.append(item)
    return leaves


def _inner_frames(error):
    frames = [frame for frame, _line in traceback.walk_tb(error.__traceback__)]
    return frames[-INNER_FRAMES:]


def _message_quotes_a_value(error, leaves):
    message = str(error)
    for leaf in leaves:
        if isinstance(leaf, str) and len(leaf) >= MIN_QUOTED_VALUE_LENGTH and leaf in message:
            return True
    return False


def _callers_object_is_a_local(error, leaves):
    # Identity, so a one-letter parameter equal to some unrelated local string is not a match.
    sent = {id(leaf) for leaf in leaves if len(leaf) >= 2 or not isinstance(leaf, (str, bytes))}
    if not sent:
        return False
    for frame in _inner_frames(error):
        for value in list(frame.f_locals.values()):
            if id(value) in sent:
                return True
    return False


# [@ANCHOR: hams_helpdesk:classify_request_input_error]
def classify_request_input_error(error, params):
    """A short reason (for the log) when ``error`` is the caller's malformed input, else None.

    ``params`` is the request's parameter dict. The exception is judged on its class, its message and where it was
    raised; see the module docstring for the four kinds of evidence. A request with no parameters at all can only
    be blamed for what the path carried, and an error with nothing from the caller to point at is not converted."""
    if not isinstance(error, INPUT_ERROR_TYPES):
        return None
    leaves = request_values(params)
    if isinstance(error, psycopg2.DataError):
        return "the database refused a value" if leaves else None
    if isinstance(error, ValueError) and _EMPTY_RECORDSET_MESSAGE.fullmatch(str(error)):
        return "no record for the value sent" if leaves else None
    if isinstance(error, (ValueError, KeyError, IndexError)) and _message_quotes_a_value(error, leaves):
        return "the message quotes a value sent"
    if _callers_object_is_a_local(error, leaves):
        return "the handler failed on a value as sent"
    return None
