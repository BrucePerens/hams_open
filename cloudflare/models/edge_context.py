# -*- coding: utf-8 -*-
from odoo import models, api
from odoo.http import request
import logging

_logger = logging.getLogger(__name__)


class CloudflareUtils(models.AbstractModel):
    _name = "cloudflare.utils"
    _description = "Cloudflare Edge Context Utilities"

    @api.model
    def get_current_website_id(self):
        """
        Unified helper to resolve the active website ID across HTTP and Cron contexts.
        """
        if request:
            request_obj = request._get_current_object()
            if request_obj.website:
                return request_obj.website.id
        return self.env["website"].get_current_website().id

    @api.model
    def get_request_context(self):
        # [@ANCHOR: cf_get_request_context]
        # Verified by [@ANCHOR: test_cf_get_request_context]
        """
        Parses Cloudflare-specific geographic and threat headers injected at the edge.
        Returns a dictionary to be used by proprietary modules for default routing.
        """
        if not request:
            return {}

        request_obj = request._get_current_object()
        headers = request_obj.httprequest.headers

        return {
            "ip": headers.get("CF-Connecting-IP")
            or request_obj.httprequest.remote_addr,
            "country": headers.get("CF-IPCountry"),
            "region": headers.get("CF-Region"),
            "city": headers.get("CF-IPCity"),
            "postal_code": headers.get("CF-Postal-Code"),
            "longitude": headers.get("CF-IPLongitude"),
            "latitude": headers.get("CF-IPLatitude"),
            "threat_score": headers.get("CF-Threat-Score"),
            "as_number": headers.get("CF-ASN"),
        }
