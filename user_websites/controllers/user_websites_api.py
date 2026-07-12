# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo import http
from odoo.http import request
import json
import logging

_logger = logging.getLogger(__name__)


class UserWebsitesApi(http.Controller):

    @http.route(
        "/api/v1/user_websites/domains",
        type="http",
        auth="public",
        methods=["GET"],
        csrf=False,
    )
    def api_domains(self, **kwargs):
        """
        Returns a list of all domains for Let's Encrypt certificate maintenance.
        This includes both user custom domains (edge.routing.domain) and,
        if installed, ham DNS zones.
        """
        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")

        all_domains = []

        # 1. Fetch edge routing domains
        edge_domains = (
            env_svc["edge.routing.domain"].search([], limit=5000).mapped("name")
        )
        all_domains.extend(edge_domains)

        # 2. Soft-depend on ham_dns
        if "ham.dns.zone" in env_svc:
            try:
                dns_env_svc = utils._get_service_env("ham_dns.user_dns_api_service")
                zone_names = (
                    dns_env_svc["ham.dns.zone"].search([], limit=5000).mapped("name")
                )
                all_domains.extend(zone_names)
            except Exception as e:  # audit-ignore-catch-all
                _logger.warning("Failed to fetch ham.dns.zone domains: %s", e)

        # Deduplicate and format
        unique_domains = list(set(all_domains))

        return request.make_response(
            json.dumps({"domains": unique_domains}),
            status=200,
            headers=[("Content-Type", "application/json")],
        )
