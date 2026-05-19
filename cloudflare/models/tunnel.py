# -*- coding: utf-8 -*-
from odoo import models, fields, api, _
from odoo.exceptions import UserError
import os
from ..utils.cloudflare_api import delete_cfd_tunnel, list_cfd_tunnels


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
        readonly=True
    )

    def action_delete_tunnel(self):
        # [@ANCHOR: cf_delete_tunnel]
        # Verified by [@ANCHOR: test_cf_delete_tunnel]
        for tunnel in self:
            token, _zone = tunnel.website_id._get_cloudflare_credentials()
            account_id = tunnel.website_id.cloudflare_account_id or os.environ.get("CLOUDFLARE_ACCOUNT_ID")

            if not token or not account_id:
                raise UserError(_("Missing Cloudflare API Token or Account ID for the website."))


            success, msg = delete_cfd_tunnel(account_id, token, tunnel.cf_tunnel_id)
            if success:
                # ADR-0001: Headless Mutation Context
                tunnel.with_context(mail_notrack=True).unlink()
            else:
                raise UserError(_("Failed to delete tunnel: %s") % msg)


    @api.model
    def action_sync_tunnels(self):
        # [@ANCHOR: cf_sync_tunnels]
        # Verified by [@ANCHOR: test_cf_sync_tunnels]
        websites = self.env["website"].search([], limit=1000)
        synced_tunnel_ids = []

        # Load all existing tunnels into a dict
        existing_tunnels = {t.cf_tunnel_id: t for t in self.env["cloudflare.tunnel"].search([], limit=10000)}
        tunnels_to_create = []

        for website in websites:
            token, _zone = website._get_cloudflare_credentials()
            account_id = website.cloudflare_account_id or os.environ.get("CLOUDFLARE_ACCOUNT_ID")

            if not token or not account_id:
                continue

            tunnels = list_cfd_tunnels(account_id, token)
            for t in tunnels:
                tunnel_id = t.get("id")
                synced_tunnel_ids.append(tunnel_id)

                created_at_raw = t.get("created_at", "")
                created_at = False
                if created_at_raw:
                    # Cloudflare returns ISO 8601 like 2021-01-01T00:00:00Z
                    created_at = created_at_raw[:19].replace('T', ' ')

                vals = {
                    "cf_tunnel_id": tunnel_id,
                    "name": t.get("name"),
                    "status": t.get("status"),
                    "created_at": created_at,
                    "website_id": website.id,
                }

                existing = existing_tunnels.get(tunnel_id)
                if existing:
                    # ADR-0001: Headless Mutation Context
                    existing.with_context(mail_notrack=True).write(vals)
                else:
                    tunnels_to_create.append(vals)

        if tunnels_to_create:
            # ADR-0001: Headless Mutation Context
            self.env["cloudflare.tunnel"].with_context(
                mail_notrack=True
            ).create(tunnels_to_create)

        return {
            "type": "ir.actions.client",
            "tag": "display_notification",
            "params": {
                "title": _("Success"),
                "message": _("Tunnels synced successfully."),
                "type": "success",
                "sticky": False,
            },
        }
