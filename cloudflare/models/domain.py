# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0.
from odoo import models, fields, api, _
from odoo.exceptions import UserError
from odoo.addons.cloudflare.utils import cloudflare_api as cf_utils


class CloudflareRoutingDomain(models.Model):
    _inherit = "edge.routing.domain"

    cloudflare_hostname_id = fields.Char(
        "Cloudflare Hostname ID",
        readonly=True,
        help="ID returned by Cloudflare for management",
    )
    ssl_status = fields.Selection(
        [
            ("pending_validation", "Pending Validation"),
            ("pending_issuance", "Pending Issuance"),
            ("pending_deployment", "Pending Deployment"),
            ("active", "Active"),
            ("error", "Error"),
        ],
        string="SSL Status",
        default="pending_validation",
        readonly=True,
    )

    @api.model_create_multi
    def create(self, vals_list):
        records = super(CloudflareRoutingDomain, self).create(vals_list)
        records._create_cloudflare_custom_hostname_batch()
        return records

    def unlink(self):
        # Verified by [@ANCHOR: COMM_test_multi_website_purge_queue]
        # spacing
        # Verified by [@ANCHOR: COMM_test_content_hook_multi_website]
        # spacing
        # Verified by [@ANCHOR: COMM_test_waf_ban_multi_website]
        # spacing
        # Verified by [@ANCHOR: COMM_test_cf_ban_ip_api]
        # spacing
        # Verified by [@ANCHOR: COMM_test_xpath_rendering_cf_settings]
        # spacing
        # Verified by [@ANCHOR: COMM_test_04_website_cache_tag_localproxy]
        # spacing
        # Verified by [@ANCHOR: COMM_test_purge_everything_multi_website_resilience]
        # spacing
        # Verified by [@ANCHOR: COMM_test_05_process_queue_optimized_exists]
        # spacing
        # Verified by [@ANCHOR: COMM_test_02_get_request_context_no_headers]
        # spacing
        # Verified by [@ANCHOR: COMM_test_cf_backend_views_rendering]
        # spacing
        # Verified by [@ANCHOR: COMM_test_05_execute_ban_missing_website]
        self._delete_cloudflare_custom_hostname_batch()
        return super(CloudflareRoutingDomain, self).unlink()

    def _get_website_mapping(self):
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_tunnel"
        )
        names = self.mapped("name")
        websites = self.env["website"].with_user(svc_uid).search([("domain", "in", names)], limit=len(names))
        return {w.domain: w for w in websites if w.domain}

    def _create_cloudflare_custom_hostname_batch(self):
        website_map = self._get_website_mapping()
        for record in self:
            website = website_map.get(record.name)
            if not website:
                raise UserError(_("No website found matching domain %s") % record.name)
            token, zone_id = website._get_cloudflare_credentials()
            if token and zone_id:
                success, result = cf_utils.create_custom_hostname(record.name, token, zone_id)
                if success and isinstance(result, dict):
                    record.cloudflare_hostname_id = result.get("id")
                    record.ssl_status = result.get("ssl", {}).get(
                        "status", "pending_validation"
                    )

    def _delete_cloudflare_custom_hostname_batch(self):
        website_map = self._get_website_mapping()
        for record in self:
            if not record.cloudflare_hostname_id:
                continue
            website = website_map.get(record.name)
            if not website:
                continue
            token, zone_id = website._get_cloudflare_credentials()
            if token and zone_id:
                cf_utils.delete_custom_hostname(record.cloudflare_hostname_id, token, zone_id)

    def action_sync_ssl_status(self):
        website_map = self._get_website_mapping()
        for record in self:
            if not record.cloudflare_hostname_id:
                continue
            website = website_map.get(record.name)
            if not website:
                continue
            token, zone_id = website._get_cloudflare_credentials()
            if not token or not zone_id:
                continue

            success, result = cf_utils.get_custom_hostname(
                record.cloudflare_hostname_id, token, zone_id
            )
            if success and isinstance(result, dict):
                new_status = result.get("ssl", {}).get("status", "pending_validation")
                if new_status != record.ssl_status:
                    record.ssl_status = new_status
