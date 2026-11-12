# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The WSGI-level scan: responses that never pass `ir.http._post_dispatch`, such as an XML-RPC fault a controller built
from an exception message. Pure WSGI plumbing; the database work is `models.secret_canary.scan_after_request`."""
import threading

from werkzeug.wsgi import ClosingIterator

from . import detector

# Set by `ir.http._post_dispatch` on the thread serving the request. Not an environ key: Odoo's `request.httprequest.environ`
# is a filtered COPY of the WSGI environ, so a key set there never reaches this wrapper (every text response the ir.http
# hook scanned was scanned a second time and raised its alarm twice).
_served = threading.local()


# [@ANCHOR: hams_secret_canary:mark_scanned]
def mark_scanned():
    """Says the request this thread is serving has had its response scanned (by `ir.http._post_dispatch`), so the WSGI
    wrapper does not scan it again."""
    _served.scanned = True


# [@ANCHOR: hams_secret_canary:is_text]
def _is_text(content_type):
    mimetype = content_type.split(";")[0].strip().lower()
    return mimetype.startswith("text/") or mimetype in detector.SCANNABLE_MIMETYPES


# [@ANCHOR: hams_secret_canary:scan_wsgi]
def scan_wsgi(application, scan):
    """Returns a WSGI callable (`(environ, start_response)`, or `(self, environ, start_response)` for a method) that
    runs `application` and passes its body through unchanged. When the response is text, was not already scanned by
    `_post_dispatch` (`mark_scanned`) and has been fully sent, calls `scan(text, path)` with its first `detector.MAX_SCAN_BYTES` bytes
    decoded. A failure in `scan` is logged and never reaches the client."""
    def call(*args):
        environ, start_response = args[-2:]
        content_type = []

        def capture(status, headers, exc_info=None):
            content_type.extend(v for k, v in headers if k.lower() == "content-type")
            return start_response(status, headers, exc_info) if exc_info else start_response(status, headers)

        _served.scanned = False
        body = application(*args[:-2], environ, capture)
        if _served.scanned or not content_type or not _is_text(content_type[0]):
            return body
        return _pass_through(body, environ.get("PATH_INFO", ""), scan)
    return call


# [@ANCHOR: hams_secret_canary:pass_through]
def _pass_through(body, path, scan):
    """The same chunks as `body`, closing `body` when the server closes the result."""
    inner = ClosingIterator(body)  # closes `body` if it has a close()
    return ClosingIterator(_scan_chunks(inner, path, scan), inner.close)


# [@ANCHOR: hams_secret_canary:scan_chunks]
def _scan_chunks(chunks, path, scan):
    seen = bytearray()
    for chunk in chunks:
        if len(seen) < detector.MAX_SCAN_BYTES:
            seen += chunk[: detector.MAX_SCAN_BYTES - len(seen)]
        yield chunk
    scan(bytes(seen).decode("utf-8", "replace"), path)
