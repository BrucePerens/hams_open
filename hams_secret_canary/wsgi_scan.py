# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The WSGI-level scan: responses that never pass `ir.http._post_dispatch`, such as an XML-RPC fault a controller built
from an exception message. Pure WSGI plumbing; the database work is `models.secret_canary.scan_after_request`."""
from werkzeug.wsgi import ClosingIterator

from . import detector

SCANNED = "hams_secret_canary.scanned"  # environ key: `ir.http._post_dispatch` already scanned this response


def _is_text(content_type):
    mimetype = content_type.split(";")[0].strip().lower()
    return mimetype.startswith("text/") or mimetype in detector.SCANNABLE_MIMETYPES


# [@ANCHOR: hams_secret_canary:scan_wsgi]
def scan_wsgi(application, scan):
    """Returns a WSGI callable (`(environ, start_response)`, or `(self, environ, start_response)` for a method) that
    runs `application` and passes its body through unchanged. When the response is text, was not already scanned by
    `_post_dispatch` and has been fully sent, calls `scan(text, path)` with its first `detector.MAX_SCAN_BYTES` bytes
    decoded. A failure in `scan` is logged and never reaches the client."""
    def call(*args):
        environ, start_response = args[-2:]
        content_type = []

        def capture(status, headers, exc_info=None):
            content_type.extend(v for k, v in headers if k.lower() == "content-type")
            return start_response(status, headers, exc_info) if exc_info else start_response(status, headers)

        body = application(*args[:-2], environ, capture)
        if environ.get(SCANNED) or not content_type or not _is_text(content_type[0]):
            return body
        return _pass_through(body, environ.get("PATH_INFO", ""), scan)
    return call


def _pass_through(body, path, scan):
    """The same chunks as `body`, closing `body` when the server closes the result."""
    inner = ClosingIterator(body)  # closes `body` if it has a close()
    return ClosingIterator(_scan_chunks(inner, path, scan), inner.close)


def _scan_chunks(chunks, path, scan):
    seen = bytearray()
    for chunk in chunks:
        if len(seen) < detector.MAX_SCAN_BYTES:
            seen += chunk[: detector.MAX_SCAN_BYTES - len(seen)]
        yield chunk
    scan(bytes(seen).decode("utf-8", "replace"), path)
