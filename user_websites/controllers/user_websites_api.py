# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
from odoo import http
from odoo.http import request
import hashlib
import hmac
import json
import logging

_logger = logging.getLogger(__name__)


# [@ANCHOR: user_websites_user_websites_api_user_websites_api]
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
        # Decision 2026-11-16: the list names every custom domain and DNS zone, so the caller must present the same
        # shared secret as pager_duty's update_domains, in the X-Api-Identity header (a query string ends up in logs).
        stored_identity = utils._get_system_param("pager_duty.domain_api_identity")
        presented = request.httprequest.headers.get("X-Api-Identity", "")
        if not stored_identity or not hmac.compare_digest(presented.encode("utf-8", "replace"), stored_identity.encode()):
            return request.make_response(
                json.dumps({"error": "Unauthorized"}),
                status=401,
                headers=[("Content-Type", "application/json")],
            )
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")

        # A single search_read(..., limit=5000) silently dropped every
        # domain past the 5000th with no signal at all -- for an endpoint
        # whose own docstring promises "a list of all domains" and whose
        # real consumer is Let's Encrypt certificate automation, a silently
        # missing domain here means that domain's certificate quietly never
        # gets renewed. Paginate by id instead (same keyset idiom used
        # elsewhere in this module, e.g. res_users.py's GDPR export loops)
        # so growth past 5000 rows can't silently drop domains again.
        def read_all_names(model_env, extra_domain=()):
            names = []
            last_id = 0
            while True:
                batch = model_env.search_read(
                    [("id", ">", last_id), *extra_domain], ["id", "name"], limit=5000, order="id asc"
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
                all_domains.extend(read_all_names(
                        dns_env_svc["ham.dns.zone"],
                        # Personal member zones are DNS-only (no site on hams1): certbot must not be asked for them.
                        [("is_personal_alias_zone", "=", False)],
                    ))
            except Exception as e:  # audit-ignore-catch-all: Tested by [@ANCHOR: test_domains_api_returns_all_domains]
                # Verified by [@ANCHOR: COMM_test_domains_api_refuses_a_partial_list]
                # A list without the DNS zones would be taken as the full one: those domains' certificates would
                # quietly stop being renewed. Refuse instead; the caller retries (edge_routing's PagerDuty push does the same).
                _logger.exception("Failed to fetch ham.dns.zone domains: %s", e)
                return request.make_response(
                    json.dumps({"error": "the DNS zone list could not be read"}),
                    status=503,
                    headers=[("Content-Type", "application/json"), ("Retry-After", "60")],
                )

        # Deduplicate and format
        unique_domains = list(set(all_domains))

        return request.make_response(
            json.dumps({"domains": unique_domains}),
            status=200,
            headers=[("Content-Type", "application/json")],
        )

    @http.route(
        "/website/unsubscribe/<string:model>/<int:record_id>/<int:partner_id>/<int(max=99999999999):timestamp>/<string:token>",
        type="http",
        auth="public",
        website=True,
        # csrf=False: the digest mail promises RFC 8058 one-click unsubscribe (List-Unsubscribe-Post), so a
        # mail provider POSTs this URL with no session and no CSRF token. The signed token in the URL is the
        # only authority and nothing here acts on a session.
        csrf=False,
    )  # fmt: skip
    # [@ANCHOR: user_websites:COMM_unsubscribe]
    def unsubscribe(self, model, record_id, partner_id, timestamp, token, **kwargs):
        # # Tested by [@ANCHOR: test_unsubscribe_secret]
        utils = request.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env("user_websites.user_websites_service_account")

        if model not in ("res.partner", "user.websites.group"):
            res_content = json.dumps({"error": "Forbidden"})
            res_headers = [("Content-Type", "application/json")]
            return request.make_response(
                res_content,
                status=403,
                headers=res_headers,
            )

        record = env_svc[model].browse(record_id)
        if not record.exists():
            res_content = json.dumps({"error": "Forbidden"})
            res_headers = [("Content-Type", "application/json")]
            return request.make_response(
                res_content,
                status=403,
                headers=res_headers,
            )

        db_secret = utils._get_crypto_secret()
        if not db_secret:
            return request.make_response(
                json.dumps({"error": "Forbidden"}),
                status=403,
                headers=[("Content-Type", "application/json")],
            )
        message = f"{model}-{record_id}-{partner_id}-{timestamp}".encode("utf-8")
        expected_token = hmac.new(
            db_secret.encode("utf-8"), message, hashlib.sha256
        ).hexdigest()

        if not hmac.compare_digest(expected_token.encode(), str(token).encode("utf-8", "replace")):
            return request.make_response(
                json.dumps({"error": "Forbidden"}),
                status=403,
                headers=[("Content-Type", "application/json")],
            )

        record.message_unsubscribe([partner_id])

        return request.render(
            "user_websites.unsubscribe_success", {"record_name": model}
        )
