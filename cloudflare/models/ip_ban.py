# -*- coding: utf-8 -*-
from odoo import models, fields, api, _
from odoo.exceptions import UserError
from odoo.http import request
from ..utils.cloudflare_api import ban_ip, unban_ip


class CloudflareIPBan(models.Model):
    _name = "cloudflare.ip.ban"
    _description = "Cloudflare IP Ban / Honeypot Registry"
    _order = "create_date desc"

    ip_address = fields.Char(string="Target IP Address", required=True)
    mode = fields.Selection(
        [
            ("block", "Block"),
            ("challenge", "Interactive Challenge"),
            ("managed_challenge", "Managed Challenge (Recommended)"),
        ],
        string="Action Applied",
        default="block",
        required=True,
    )
    notes = fields.Char(string="Trigger Reason", default="Honeypot Triggered")
    cf_rule_id = fields.Char(string="Cloudflare Rule ID", readonly=True)
    state = fields.Selection(
        [
            ("active", "Active (Banned)"),
            ("lifted", "Lifted (Unbanned)"),
            ("failed", "API Sync Failed"),
        ],
        string="Status",
        default="active",
    )
    website_id = fields.Many2one(
        "website",
        string="Website",
        default=lambda self: self.env["website"].get_current_website().id,
    )

    _ip_website_uniq = models.Constraint(
        "UNIQUE(ip_address, website_id)", "This IP is already banned for this website!"
    )
    _ip_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(ip_address)) > 0)", "The IP address cannot be empty."
    )

    @api.model
    def _execute_ban(
        self, ip_address, mode="block", notes="Honeypot Triggered", website_id=None
    ):
        # [@ANCHOR: cf_execute_ban]
        # Verified by [@ANCHOR: test_cf_execute_ban]
        if not website_id:
            try:
                if request and getattr(request, "website", False):
                    website_id = request.website.id
                else:
                    website_id = self.env["website"].get_current_website().id
            except RuntimeError:
                website_id = self.env["website"].get_current_website().id

        website = self.env["website"].browse(website_id)
        token, zone_id = website._get_cloudflare_credentials()

        success, result = ban_ip(ip_address, mode, notes, token, zone_id)

        if success:
            # ADR-0001: Headless Mutation Context
            self.env["cloudflare.ip.ban"].with_context(
                mail_notrack=True, prefetch_fields=False
            ).create(
                {
                    "ip_address": ip_address,
                    "mode": mode,
                    "notes": notes,
                    "cf_rule_id": result,
                    "state": "active",
                    "website_id": website.id,
                }
            )
            return True
        else:
            # ADR-0001: Headless Mutation Context
            self.env["cloudflare.ip.ban"].with_context(
                mail_notrack=True, prefetch_fields=False
            ).create(
                {
                    "ip_address": ip_address,
                    "mode": mode,
                    "notes": f"Failed to deploy: {result}",
                    "state": "failed",
                    "website_id": website.id,
                }
            )
            return False

    def action_lift_ban(self):
        # [@ANCHOR: cf_action_lift_ban]
        # Verified by [@ANCHOR: test_cf_action_lift_ban]

        for rec in self:
            if rec.state == "active" and rec.cf_rule_id:
                token, zone_id = (
                    rec.website_id._get_cloudflare_credentials()
                    if rec.website_id
                    else (None, None)
                )
                success, msg = unban_ip(rec.cf_rule_id, token, zone_id)
                if success:
                    # ADR-0001: Headless Mutation Context
                    rec.with_context(mail_notrack=True, prefetch_fields=False).state = "lifted"
                else:
                    raise UserError(
                        _("Failed to lift ban via Cloudflare API: %s") % msg
                    )
