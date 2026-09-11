# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
from odoo import models, api, fields
from odoo.http import request



class CloudflareUtils(models.AbstractModel):
    _name = "cloudflare.utils"
    _description = "Cloudflare Edge Context Utilities"
    name = fields.Char(string="Name", default=lambda self: self._description)

    @api.model
    # [@ANCHOR: cloudflare:COMM_get_current_website_id]
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
        # [@ANCHOR: COMM_cf_get_request_context]

        # # Verified by [@ANCHOR: COMM_test_cf_get_request_context]
        """
        Parses Cloudflare-specific geographic and threat headers injected at the edge.
        Returns a dictionary to be used by proprietary modules for default routing.

        Bug-hunt fix, 2026-09-11 (see
        docs/bug_hunt_claims/hams_open/cloudflare/models/claims/cf_get_request_context.md):
        this used to trust every CF-* header unconditionally, with no check
        that the request actually transited Cloudflare's edge -- forgeable
        by any direct request to the origin. This deployment is
        Tunnel-only (confirmed by Bruce 2026-09-11), so CF-* headers are
        only genuine when the request's real transport peer is loopback
        (cloudflared and Odoo run on the same host). When it isn't, every
        CF-* field is dropped rather than trusted -- an attacker who
        reaches origin directly gets no CF-derived geo/threat data at all,
        not forged data.
        """
        if not request:
            return {}

        request_obj = request._get_current_object()
        headers = request_obj.httprequest.headers
        real_ip = self.env["zero_sudo.security.utils"]._get_trusted_client_ip(request_obj)
        via_tunnel = request_obj.httprequest.remote_addr in ("127.0.0.1", "::1")  # burn-ignore-tunnel-peer-check

        if not via_tunnel:
            return {
                "ip": real_ip,
                "country": None,
                "region": None,
                "city": None,
                "postal_code": None,
                "longitude": None,
                "latitude": None,
                "threat_score": None,
                "as_number": None,
            }

        return {
            "ip": real_ip,
            "country": headers.get("CF-IPCountry"),
            "region": headers.get("CF-Region"),
            "city": headers.get("CF-IPCity"),
            "postal_code": headers.get("CF-Postal-Code"),
            "longitude": headers.get("CF-IPLongitude"),
            "latitude": headers.get("CF-IPLatitude"),
            "threat_score": headers.get("CF-Threat-Score"),
            "as_number": headers.get("CF-ASN"),
        }
