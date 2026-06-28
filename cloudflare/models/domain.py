# -*- coding: utf-8 -*-
from odoo import models, fields, api


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
        for record in records:
            record._create_cloudflare_custom_hostname()
        return records

    def unlink(self):
        for record in self:
            if record.cloudflare_hostname_id:
                record._delete_cloudflare_custom_hostname()
        return super(CloudflareRoutingDomain, self).unlink()

    def _create_cloudflare_custom_hostname(self):
        self.ensure_one()
        cf_utils = self.env["cloudflare.utils"].with_user(
            self.env.ref("base.user_admin")
        )
        config = self.env["ir.config_parameter"].with_user(
            self.env.ref("base.user_admin")
        )
        token = config.get_param("cloudflare.api_token")
        zone_id = config.get_param("cloudflare.zone_id")

        if token and zone_id:
            success, result = cf_utils.create_custom_hostname(self.name, token, zone_id)
            if success and isinstance(result, dict):
                self.cloudflare_hostname_id = result.get("id")
                self.ssl_status = result.get("ssl", {}).get(
                    "status", "pending_validation"
                )

    def _delete_cloudflare_custom_hostname(self):
        self.ensure_one()
        cf_utils = self.env["cloudflare.utils"].with_user(
            self.env.ref("base.user_admin")
        )
        config = self.env["ir.config_parameter"].with_user(
            self.env.ref("base.user_admin")
        )
        token = config.get_param("cloudflare.api_token")
        zone_id = config.get_param("cloudflare.zone_id")

        if token and zone_id:
            cf_utils.delete_custom_hostname(self.cloudflare_hostname_id, token, zone_id)

    def action_sync_ssl_status(self):
        for record in self:
            if not record.cloudflare_hostname_id:
                continue

            cf_utils = self.env["cloudflare.utils"].with_user(
                self.env.ref("base.user_admin")
            )
            config = self.env["ir.config_parameter"].with_user(
                self.env.ref("base.user_admin")
            )
            token = config.get_param("cloudflare.api_token")
            zone_id = config.get_param("cloudflare.zone_id")

            if token and zone_id:
                success, result = cf_utils.get_custom_hostname(
                    record.cloudflare_hostname_id, token, zone_id
                )
                if success and isinstance(result, dict):
                    new_status = result.get("ssl", {}).get(
                        "status", "pending_validation"
                    )
                    if new_status != record.ssl_status:
                        record.ssl_status = new_status
