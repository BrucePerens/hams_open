# -*- coding: utf-8 -*-
"""The XML-RPC endpoints never put a traceback or an unexpected exception's text in a reply.

QA pass 2 (2026-10-14): `database_lockdown` scrubs the JSON-RPC and JSON-2 error bodies, but Odoo's `/xmlrpc/<service>` and
`/xmlrpc/2/<service>` answer an unexpected exception with the whole Python traceback as the fault string, to anyone, signed
in or not: a malformed request body returned library and source paths and line numbers. Expected errors (UserError,
AccessError, AccessDenied, MissingError, RedirectWarning) keep their message, which is written for the caller; everything
else becomes "Odoo Server Error" and is still logged in full by Odoo. In developer mode the stock reply is kept. The routes
and their behaviour are otherwise the core's: only the two functions that build a fault reply are replaced.
"""
import sys
import xmlrpc.client

from odoo.exceptions import AccessDenied, RedirectWarning, UserError

from odoo.addons.rpc.controllers.xmlrpc import (
    RPC_FAULT_CODE_APPLICATION_ERROR,
    XMLRPC,
    dumps,
    xmlrpc_handle_exception_int,
    xmlrpc_handle_exception_string,
)

from .database_lockdown import GENERIC_MESSAGE, developer_mode

# AccessError and MissingError are UserErrors.
EXPECTED_XMLRPC_ERRORS = (UserError, AccessDenied, RedirectWarning)


# [@ANCHOR: hams_base:xmlrpc_fault_scrub]
def scrubbed_fault_int(error):
    """The `/xmlrpc/2/` reply for `error`: the core's own for an expected error, a generic fault otherwise."""
    if developer_mode() or isinstance(error, EXPECTED_XMLRPC_ERRORS):
        return xmlrpc_handle_exception_int(error)
    return dumps(xmlrpc.client.Fault(RPC_FAULT_CODE_APPLICATION_ERROR, GENERIC_MESSAGE))


def scrubbed_fault_string(error):
    """The legacy `/xmlrpc/` reply (the fault code is a string) for `error`."""
    if developer_mode() or isinstance(error, EXPECTED_XMLRPC_ERRORS):
        return xmlrpc_handle_exception_string(error)
    return dumps(xmlrpc.client.Fault(GENERIC_MESSAGE, ""))


# `xmlrpc_1` and `xmlrpc_2` build their error replies by calling these two module-level functions by name, and Odoo
# registers the stock routes of the `rpc` module alongside a subclass that would override them (a subclass of
# `XMLRPC` did not take over: the stock route still matched first), so the scrubbing is installed where the
# stock methods look the functions up. `database_lockdown` could subclass the dispatchers instead because they are
# registered by routing type; these two functions have no such registry.
def install():
    stock = sys.modules[XMLRPC.__module__]
    stock.xmlrpc_handle_exception_int = scrubbed_fault_int
    stock.xmlrpc_handle_exception_string = scrubbed_fault_string


install()
