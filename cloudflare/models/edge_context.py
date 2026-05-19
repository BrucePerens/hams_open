# -*- coding: utf-8 -*-
from odoo import models, api
from odoo.http import request


class CloudflareUtils(models.AbstractModel):
    _name = "cloudflare.utils"
    _description = "Cloudflare Edge Context Utilities"

    @api.model
    def get_current_website_id(self):
        """
        Unified helper to resolve the active website ID across HTTP and Cron contexts.
        """
        try:
            if request and getattr(request, "website", False):
                return request.website.id
        except RuntimeError:
            pass
        return self.env["website"].get_current_website().id

    @api.model
    def get_request_context(self):
        # [@ANCHOR: cf_get_request_context]
        """
        Parses Cloudflare-specific geographic and threat headers injected at the edge.
        Returns a dictionary to be used by proprietary modules for default routing.
        """
        try:
            if not request or not hasattr(request, "httprequest"):
                return {}
            headers = request.httprequest.headers
        except RuntimeError:
            return {}

        return {
            "ip": headers.get("CF-Connecting-IP") or request.httprequest.remote_addr,
            "country": headers.get("CF-IPCountry"),
            "region": headers.get("CF-Region"),
            "city": headers.get("CF-IPCity"),
            "postal_code": headers.get("CF-Postal-Code"),
            "longitude": headers.get("CF-IPLongitude"),
            "latitude": headers.get("CF-IPLatitude"),
            "threat_score": headers.get("CF-Threat-Score"),
            "as_number": headers.get("CF-ASN"),
        }
