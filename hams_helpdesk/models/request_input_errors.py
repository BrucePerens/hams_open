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
  handler that called ``.split()`` or ``.get()`` on a parameter that arrived as the wrong type;
* Odoo's own parsing of a request parameter refused the value as sent: ``str2bool`` called from a controller on a
  value the request carried (``/web/content/<id>/<file>?download=514750499``, ticket 103), or an ``assert`` in one of
  Odoo's own controllers on a parameter as sent (``/web/assets/...?assets_params=x``: "assert isinstance(assets_params,
  dict)"). See ``_odoo_refused_a_parameter``.

A TypeError or AttributeError is accepted only on the fourth of these, because their messages never quote a value and
a bare TypeError is far more often a code bug than a caller's mistake. An AssertionError is accepted only on the last,
and only from Odoo's own controllers: an assert in our code is our bug.
"""

import ast
import linecache
import os
import re
import traceback

import psycopg2

from odoo.tools import misc as odoo_misc

# The classes converted on the generic evidence. Everything else (UserError, AccessError, NameError, RuntimeError,
# QWebError, OperationalError, IntegrityError, ...) is left alone, except an AssertionError from one of Odoo's own
# controllers, see _odoo_refused_a_parameter.
INPUT_ERROR_TYPES = (
    ValueError,  # includes UnicodeError and json.JSONDecodeError
    KeyError,
    IndexError,
    TypeError,
    AttributeError,
    OverflowError,
    psycopg2.DataError,
)
# What ir.http._dispatch catches and hands to classify_request_input_error; anything not classified is re-raised as is.
CAUGHT_ERROR_TYPES = INPUT_ERROR_TYPES + (AssertionError,)

# A value shorter than this is not looked for inside a message: "a" is in every message.
MIN_QUOTED_VALUE_LENGTH = 3
# How many of the innermost frames are searched for the caller's own object. The handler or its parsing helper is
# at the bottom of the traceback; frames above are Odoo's dispatch machinery, whose locals hold the whole request.
INNER_FRAMES = 3
# What ensure_one() says of an empty recordset: "Expected singleton: res.users()". A recordset of two or more prints
# its ids ("res.users(1, 2)"), which is a different, real, bug and does not match.
_EMPTY_RECORDSET_MESSAGE = re.compile(r"Expected singleton: [\w.]+\(\)")

# What odoo.tools.misc.str2bool raises for text that is not one of its words. content_common and content_image
# (/web/content, /web/image) call it on ?download= before anything else.
STR2BOOL_MESSAGE = "Use 0/1/yes/no/true/false/on/off"
_STR2BOOL_FILE = os.path.realpath(odoo_misc.__file__)
# The directory Odoo is installed in (dist-packages): its stock modules live below it, in odoo/addons/ or addons/,
# and ours never do.
# (odoo is a namespace package with no __file__, so this is found from odoo/tools/misc.py.)
_ODOO_DISTRIBUTION = os.path.dirname(os.path.dirname(os.path.dirname(_STR2BOOL_FILE))) + os.sep


# [@ANCHOR: hams_helpdesk:request_input_values]
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


# [@ANCHOR: hams_helpdesk:input_error_inner_frames]
def _inner_frames(error):
    frames = [frame for frame, _line in traceback.walk_tb(error.__traceback__)]
    return frames[-INNER_FRAMES:]


# [@ANCHOR: hams_helpdesk:input_error_message_quotes_a_value]
def _message_quotes_a_value(error, leaves):
    message = str(error)
    for leaf in leaves:
        if isinstance(leaf, str) and len(leaf) >= MIN_QUOTED_VALUE_LENGTH and leaf in message:
            return True
    return False


# [@ANCHOR: hams_helpdesk:input_error_callers_object_is_a_local]
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


_CONTROLLERS_DIRECTORY = f"{os.sep}controllers{os.sep}"


# [@ANCHOR: hams_helpdesk:input_error_assert_names]
def _assert_names(frame, line_number):
    """The names an ``assert`` statement on ``line_number`` of ``frame``'s file reads; empty when the line is not one."""
    source = linecache.getline(frame.f_code.co_filename, line_number, frame.f_globals).strip()
    try:
        statement = ast.parse(source).body[0]
    except (SyntaxError, IndexError, ValueError):
        return set()
    if not isinstance(statement, ast.Assert):
        return set()
    return {node.id for node in ast.walk(statement.test) if isinstance(node, ast.Name)}


# [@ANCHOR: hams_helpdesk:input_error_odoo_parameter_parsing]
def _odoo_refused_a_parameter(error, leaves):
    """A reason when Odoo's own parsing of a request parameter refused the value the caller sent, else None.

    * ``str2bool``: the exact message, raised in odoo/tools/misc.py's ``str2bool``, called from a controller, on a
      value the request carried. ``str2bool`` lower-cases its argument before it raises, so the value is compared,
      not looked up by identity; an empty or one-letter string carries no identity anyway (CPython shares them),
      which is why the generic identity rule missed ``?download=`` and ``?download=x``.
    * ``assert`` in one of Odoo's stock controllers whose condition reads a local that is the very object the caller
      sent (``assert isinstance(assets_params, dict)`` for ``?assets_params=x``). An assert on a value the handler
      built (``assert not '%' in url``) or anywhere in our own code is not converted."""
    if not leaves:
        return None
    frames = list(traceback.walk_tb(error.__traceback__))
    if not frames:
        return None
    innermost, innermost_line = frames[-1]
    if isinstance(error, ValueError) and str(error) == STR2BOOL_MESSAGE:
        code = innermost.f_code
        if code.co_name != "str2bool" or os.path.realpath(code.co_filename) != _STR2BOOL_FILE or len(frames) < 2:
            return None
        if _CONTROLLERS_DIRECTORY not in frames[-2][0].f_code.co_filename:
            return None
        refused = innermost.f_locals.get("s")
        for leaf in leaves:
            if leaf is refused or (isinstance(leaf, str) and isinstance(refused, str) and leaf.lower() == refused):
                return "Odoo could not read a yes/no parameter"
        return None
    filename = innermost.f_code.co_filename
    in_an_odoo_controller = _CONTROLLERS_DIRECTORY in filename and os.path.realpath(filename).startswith(_ODOO_DISTRIBUTION)
    if type(error) is AssertionError and in_an_odoo_controller:
        sent = {id(leaf) for leaf in leaves}
        local_values = innermost.f_locals
        for name in _assert_names(innermost, innermost_line):
            if name in local_values and id(local_values[name]) in sent:
                return "an Odoo controller refused a parameter as sent"
    return None


# [@ANCHOR: hams_helpdesk:classify_request_input_error]
def classify_request_input_error(error, params):
    """A short reason (for the log) when ``error`` is the caller's malformed input, else None.

    ``params`` is the request's parameter dict. The exception is judged on its class, its message and where it was
    raised; see the module docstring for the five kinds of evidence. A request with no parameters at all can only
    be blamed for what the path carried, and an error with nothing from the caller to point at is not converted."""
    leaves = request_values(params)
    odoo_reason = _odoo_refused_a_parameter(error, leaves)
    if odoo_reason:
        return odoo_reason
    if not isinstance(error, INPUT_ERROR_TYPES):
        return None
    if isinstance(error, psycopg2.DataError):
        return "the database refused a value" if leaves else None
    if isinstance(error, ValueError) and _EMPTY_RECORDSET_MESSAGE.fullmatch(str(error)):
        return "no record for the value sent" if leaves else None
    if isinstance(error, (ValueError, KeyError, IndexError)) and _message_quotes_a_value(error, leaves):
        return "the message quotes a value sent"
    if _callers_object_is_a_local(error, leaves):
        return "the handler failed on a value as sent"
    return None
