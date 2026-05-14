# -*- coding: utf-8 -*-
from odoo import models, fields, _
from odoo.exceptions import UserError

class CloudflarePurgeWizard(models.TransientModel):
    _name = "cloudflare.purge.wizard"
    _description = "Cloudflare Manual Cache Purge Wizard"

    website_id = fields.Many2one(
        "website",
        string="Website",
        default=lambda self: self.env["website"].get_current_website().id,
        required=True
    )
    purge_type = fields.Selection(
        [
            ("everything", "Purge Everything"),
            ("urls", "Purge Specific URLs"),
            ("tags", "Purge Specific Cache-Tags")
        ],
        string="Purge Type",
        default="everything",
        required=True
    )
    items_to_purge = fields.Text(
        string="Items to Purge",
        help="Enter URLs or Cache-Tags separated by a new line or comma."
    )

    def action_purge(self):
        self.ensure_one()
        token, zone_id = self.website_id._get_cloudflare_credentials()
        if not token or not zone_id:
            raise UserError(_("Missing Cloudflare API Token or Zone ID for the selected website."))

        from ..utils.cloudflare_api import purge_everything, purge_urls, purge_tags  # noqa: E402

        if self.purge_type == "everything":
            success = purge_everything(token, zone_id)
            if success:
                return {
                    "type": "ir.actions.client",
                    "tag": "display_notification",
                    "params": {
                        "title": _("Success"),
                        "message": _("Purged everything from Cloudflare cache successfully."),
                        "type": "success",
                        "sticky": False,
                    },
                }
            else:
                raise UserError(_("Failed to purge everything from Cloudflare cache."))

        items = [i.strip() for i in self.items_to_purge.replace(',', '\n').split('\n') if i.strip()]
        if not items:
            raise UserError(_("Please provide items to purge."))

        if self.purge_type == "urls":
            # Normalizing URLs based on website domain if they are relative
            base_url = self.website_id.domain.rstrip('/') if self.website_id.domain else self.env["zero_sudo.security.utils"]._get_system_param("web.base.url", "https://odoo").rstrip("/")
            # ADR-0001: Standardized URL resolution
            normalized_urls = [f"{base_url}{u}" if str(u).startswith("/") else u for u in items]
            success = purge_urls(normalized_urls, token, zone_id)
            if success:
                return {
                    "type": "ir.actions.client",
                    "tag": "display_notification",
                    "params": {
                        "title": _("Success"),
                        "message": _("Purged specific URLs from Cloudflare cache successfully."),
                        "type": "success",
                        "sticky": False,
                    },
                }
            else:
                raise UserError(_("Failed to purge specific URLs from Cloudflare cache."))

        elif self.purge_type == "tags":
            success = purge_tags(items, token, zone_id)
            if success:
                return {
                    "type": "ir.actions.client",
                    "tag": "display_notification",
                    "params": {
                        "title": _("Success"),
                        "message": _("Purged specific Cache-Tags from Cloudflare cache successfully."),
                        "type": "success",
                        "sticky": False,
                    },
                }
            else:
                raise UserError(_("Failed to purge specific Cache-Tags from Cloudflare cache."))
