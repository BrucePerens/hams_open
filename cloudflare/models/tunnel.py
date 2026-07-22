# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0.
from odoo import models, fields, api, _
from odoo.exceptions import UserError
from ..utils.cloudflare_api import delete_cfd_tunnel, list_cfd_tunnels, get_cfd_tunnel_token
from ..utils.cloudflare_daemon import start_tunnel_daemon


class CloudflareTunnel(models.Model):
    _name = "cloudflare.tunnel"
    _description = "Cloudflare Tunnel"

    cf_tunnel_id = fields.Char(string="Tunnel ID", readonly=True, required=True)
    name = fields.Char(string="Tunnel Name", readonly=True, required=True)
    status = fields.Char(string="Status", readonly=True)
    created_at = fields.Datetime(string="Created At", readonly=True)
    website_id = fields.Many2one(
        "website",
        string="Website",
        default=lambda self: self.env["website"].get_current_website().id,
        readonly=True,
    )
    route_ids = fields.One2many(
        "cloudflare.tunnel.route", "tunnel_id", string="Routing Table"
    )

    def action_push_configuration(self):
        # We need update_cfd_tunnel_configuration from cloudflare_api
        from ..utils.cloudflare_api import update_cfd_tunnel_configuration
        for tunnel in self:
            token, _zone = tunnel.website_id._get_cloudflare_credentials()
            account_id = tunnel.website_id.cloudflare_account_id

            if not token or not account_id:
                raise UserError(
                    _("Missing Cloudflare API Token or Account ID for the website.")
                )

            global_routes = self.env["cloudflare.tunnel.route"].search([("tunnel_id", "=", False)])
            all_routes = tunnel.route_ids | global_routes

            ingress = []
            for route in all_routes.sorted('sequence'):
                rule = {"service": route.service_url}
                if route.hostname:
                    rule["hostname"] = route.hostname
                if route.path:
                    rule["path"] = route.path
                ingress.append(rule)
            
            # Catch-all required by Cloudflare
            ingress.append({"service": "http://localhost:8069"})

            payload = {"config": {"ingress": ingress}}
            success, msg = update_cfd_tunnel_configuration(
                account_id, token, tunnel.cf_tunnel_id, payload
            )
            if not success:
                raise UserError(_("Failed to push configuration: %s") % msg)
            
            # Simple notification since mail.thread isn't used
            return {
                "type": "ir.actions.client",
                "tag": "display_notification",
                "params": {
                    "title": _("Success"),
                    "message": _("Successfully pushed configuration to Cloudflare."),
                    "type": "success",
                    "sticky": False,
                },
            }

    def action_delete_tunnel(self):
        # [@ANCHOR: COMM_cf_delete_tunnel]

        # # Verified by [@ANCHOR: COMM_test_cf_delete_tunnel]
        tunnels_to_unlink = self.env["cloudflare.tunnel"]
        for tunnel in self:
            token, _zone = tunnel.website_id._get_cloudflare_credentials()
            account_id = tunnel.website_id.cloudflare_account_id

            if not token or not account_id:
                raise UserError(
                    _("Missing Cloudflare API Token or Account ID for the website.")
                )

            success, msg = delete_cfd_tunnel(account_id, token, tunnel.cf_tunnel_id)
            if success:
                tunnels_to_unlink |= tunnel
            else:
                raise UserError(_("Failed to delete tunnel: %s") % msg)
        
        if tunnels_to_unlink:
            # ADR-0001: Headless Mutation Context
            tunnels_to_unlink.unlink()

    @api.model
    def action_sync_tunnels(self):
        # [@ANCHOR: COMM_cf_sync_tunnels]

        # # Verified by [@ANCHOR: COMM_test_cf_sync_tunnels]
        websites = self.env["website"].search([], limit=1000)
        for website in websites:
            # We sync synchronously because this is called via cron or manually, and we don't have queue_job.
            self._sync_tunnels_for_website(website.id)

        return {
            "type": "ir.actions.client",
            "tag": "display_notification",
            "params": {
                "title": _("Success"),
                "message": _("Tunnels sync queued successfully."),
                "type": "success",
                "sticky": False,
            },
        }

    @api.model
    def _sync_tunnels_for_website(self, website_id):
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_tunnel"
        )
        self = self.with_user(svc_uid)

        website = self.env["website"].browse(website_id)
        if not website.exists():
            return

        token, _zone = website._get_cloudflare_credentials()
        account_id = website.cloudflare_account_id

        if not token or not account_id:
            return

        tunnels = list_cfd_tunnels(account_id, token)
        existing_tunnels = {
            t.cf_tunnel_id: t
            for t in self.env["cloudflare.tunnel"].search(
                [("website_id", "=", website.id)], limit=10000
            )
        }

        tunnels_to_create = []
        for t in tunnels:
            tunnel_id = t.get("id")

            created_at_raw = t.get("created_at", "")
            created_at = False
            if created_at_raw:
                created_at = created_at_raw[:19].replace("T", " ")

            vals = {
                "cf_tunnel_id": tunnel_id,
                "name": t.get("name"),
                "status": t.get("status"),
                "created_at": created_at,
                "website_id": website.id,
            }

            existing = existing_tunnels.get(tunnel_id)
            if existing:
                existing.write(vals)
            else:
                tunnels_to_create.append(vals)

        if tunnels_to_create:
            self.env["cloudflare.tunnel"].create(tunnels_to_create)

    @api.model
    def action_ensure_tunnel_running(self):
        # Find the primary tunnel for the current website
        # In a single-server setup, we just pick the first tunnel available.
        tunnel = self.env["cloudflare.tunnel"].search([], limit=1)
        if not tunnel:
            return

        token, _zone = tunnel.website_id._get_cloudflare_credentials()
        account_id = tunnel.website_id.cloudflare_account_id

        if not token or not account_id:
            return

        success, tunnel_token = get_cfd_tunnel_token(account_id, token, tunnel.cf_tunnel_id)
        if success and tunnel_token:
            start_tunnel_daemon(tunnel_token)
