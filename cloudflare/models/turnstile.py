# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
from odoo import models, api, fields
from ..utils.cloudflare_api import verify_turnstile


class CloudflareTurnstile(models.AbstractModel):
    _name = "cloudflare.turnstile"
    _description = "Cloudflare Turnstile Interface"
    name = fields.Char(string="Name", default=lambda self: self._description)

    @api.model
    def verify_token(self, token, remote_ip=None, website_id=None):
        # [@ANCHOR: COMM_cf_turnstile_verify]
        # Bug fix (bug-hunt, review_tier 1, 2026-09-09): verify_token is
        # meant to be callable by an anonymous/public site visitor
        # submitting a Turnstile CAPTCHA response -- that's the entire
        # purpose of CAPTCHA verification. But `cloudflare_turnstile_secret`
        # carries a `groups=` restriction (base.group_system plus the three
        # cloudflare service groups), and Odoo enforces that restriction on
        # every Python-level field read via the field descriptor's own
        # __get__, for whichever `self.env.user` is actually current -- not
        # just via the RPC/ACL layer. Reading it here with no elevation
        # meant an ordinary anonymous caller would get AccessError instead
        # of a clean True/False verdict, breaking CAPTCHA verification for
        # exactly the population it exists to check. .sudo() is forbidden
        # on this platform (see tunnel.py's action_ensure_tunnel_running
        # comment); elevating to the cloudflare.user_cloudflare_waf service
        # account instead is safe specifically because this function only
        # ever returns a boolean verification verdict to its caller, never
        # the secret itself -- unlike the credential-serving bugs found
        # elsewhere in this module, there is nothing here for an
        # unprivileged caller to extract by triggering this elevation.
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_waf"
        )
        if not website_id:
            website_id = self.env["cloudflare.utils"].get_current_website_id()

        website = self.env["website"].with_user(svc_uid).browse(website_id)
        secret = website.cloudflare_turnstile_secret

        if not secret:
            return False

        return verify_turnstile(token, remote_ip, secret)
