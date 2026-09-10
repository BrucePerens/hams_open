# SPDX-License-Identifier: AGPL-3.0-or-later

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
    # [@ANCHOR: user_websites:COMM_api_domains]
    def api_domains(self, **kwargs):
        # # Verified by [@ANCHOR: COMM_test_domains_api_returns_all_domains]
        """
        Returns a list of all domains for Let's Encrypt certificate maintenance.
        This includes both user custom domains (edge.routing.domain) and,
        if installed, ham DNS zones.
        """
        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")

        # A single search_read(..., limit=5000) silently dropped every
        # domain past the 5000th with no signal at all -- for an endpoint
        # whose own docstring promises "a list of all domains" and whose
        # real consumer is Let's Encrypt certificate automation, a silently
        # missing domain here means that domain's certificate quietly never
        # gets renewed. Paginate by id instead (same keyset idiom used
        # elsewhere in this module, e.g. res_users.py's GDPR export loops)
        # so growth past 5000 rows can't silently drop domains again.
        def read_all_names(model_env):
            names = []
            last_id = 0
            while True:
                batch = model_env.search_read(
                    [("id", ">", last_id)], ["id", "name"], limit=5000, order="id asc"
                )
                if not batch:
                    break
                names.extend(row["name"] for row in batch)
                last_id = batch[-1]["id"]
                if len(batch) < 5000:
                    break
            return names

        all_domains = []

        # 1. Fetch edge routing domains
        all_domains.extend(read_all_names(env_svc["edge.routing.domain"]))

        # 2. Soft-depend on ham_dns
        if "ham.dns.zone" in env_svc:
            try:
                dns_env_svc = utils._get_service_env("ham_dns.user_dns_api_service")
                all_domains.extend(read_all_names(dns_env_svc["ham.dns.zone"]))
            except Exception as e:  # audit-ignore-catch-all: Tested by [@ANCHOR: test_domains_api_returns_all_domains]
                _logger.exception("Failed to fetch ham.dns.zone domains: %s", e)

        # Deduplicate and format
        unique_domains = list(set(all_domains))

        return request.make_response(
            json.dumps({"domains": unique_domains}),
            status=200,
            headers=[("Content-Type", "application/json")],
        )
