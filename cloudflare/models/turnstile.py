# -*- coding: utf-8 -*-
from odoo import models, api, fields
from ..utils.cloudflare_api import verify_turnstile


class CloudflareTurnstile(models.AbstractModel):
    _name = "cloudflare.turnstile"
    _description = "Cloudflare Turnstile Interface"
    name = fields.Char(string="Name", default=lambda self: self._description)

    @api.model
    def verify_token(self, token, remote_ip=None, website_id=None):
        # [@ANCHOR: cf_turnstile_verify]
        if not website_id:
            website_id = self.env["cloudflare.utils"].get_current_website_id()

        website = self.env["website"].browse(website_id)
        secret = website.cloudflare_turnstile_secret

        if not secret:
            return False

        return verify_turnstile(token, remote_ip, secret)
