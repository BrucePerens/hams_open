# SPDX-License-Identifier: AGPL-3.0-or-later
"""State that lives exactly as long as one HTTP request.

Kept in a weak map keyed by the werkzeug request object instead of as attributes poked onto Odoo's
request proxy, so there is no attribute to be missing and nothing outlives the request."""

import weakref

from odoo.http import request

_STATES = weakref.WeakKeyDictionary()


# [@ANCHOR: tenant_sites:COMM_request_state]
# Verified by [@ANCHOR: tenant_sites:COMM_test_request_state]
def state():
    """The dict of the request being served, or None when no request is being served."""
    if not request:
        return None
    return _STATES.setdefault(request.httprequest, {})
