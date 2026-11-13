# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
import logging

from odoo import models
from odoo.http import request

from .. import detector, wsgi_scan

_logger = logging.getLogger(__name__)


# [@ANCHOR: hams_secret_canary_ir_http_ir_http]
class IrHttp(models.AbstractModel):
    _inherit = "ir.http"

    @classmethod
    # [@ANCHOR: hams_secret_canary:post_dispatch_scan]
    def _post_dispatch(cls, response):
        """Scans a finished text response for a canary or a registered secret and raises the alarm. Never changes the
        response: the leak has already happened, this only makes it noticed. Skips responses that are not text or are
        streamed."""
        result = super()._post_dispatch(response)
        if request and detector.scannable_response(response):
            wsgi_scan.mark_scanned()
            try:
                data = response.get_data()[: detector.MAX_SCAN_BYTES]
                request.env["hams.secret.canary"]._scan_text(
                    data.decode("utf-8", "replace"), "response", request.httprequest.path
                )
            except Exception:  # audit-ignore-catch-all: a detector must not break the response it is watching
                # E.g. an XML-RPC call, whose request environment can hold a closed cursor by now (in test mode always).
                _logger.exception("Secret canary: scanning a response failed")
        return result
