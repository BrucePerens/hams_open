# -*- coding: utf-8 -*-
from odoo import models, api
from odoo.http import request
from ..utils.cloudflare_api import verify_turnstile


class CloudflareTurnstile(models.AbstractModel):
    _name = "cloudflare.turnstile"
    _description = "Cloudflare Turnstile Interface"

    @api.model
    def verify_token(self, token, remote_ip=None, website_id=None):
        # [@ANCHOR: cf_turnstile_verify]
        if not website_id:

            try:
                if getattr(request, "website", False):
                    website_id = request.website.id
                else:
                    website_id = self.env["website"].get_current_website().id
            except RuntimeError:
                website_id = self.env["website"].get_current_website().id

        website = self.env["website"].browse(website_id)
        secret = website.cloudflare_turnstile_secret

        if not secret:
            return False

        return verify_turnstile(token, remote_ip, secret)
