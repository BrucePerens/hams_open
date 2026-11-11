# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import models
from odoo.http import request

from .. import detector, wsgi_scan


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
            request.httprequest.environ[wsgi_scan.SCANNED] = True
            data = response.get_data()[: detector.MAX_SCAN_BYTES]
            request.env["hams.secret.canary"]._scan_text(
                data.decode("utf-8", "replace"), "response", request.httprequest.path
            )
        return result
