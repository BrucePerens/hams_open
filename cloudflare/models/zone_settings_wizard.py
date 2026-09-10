# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
from odoo import models, fields, api, _
from odoo.exceptions import UserError
from ..utils.cloudflare_api import get_zone_settings, update_zone_setting


class CloudflareZoneSettingsWizard(models.TransientModel):
    _name = "cloudflare.zone.settings.wizard"
    _description = "Cloudflare Zone Settings Wizard"
    name = fields.Char(string="Name", default=lambda self: self._description)

    website_id = fields.Many2one(
        "website",
        string="Website",
        default=lambda self: self.env["website"].get_current_website().id,
        required=True,
    )
    security_level = fields.Selection(
        [
            ("essentially_off", "Essentially Off"),
            ("low", "Low"),
            ("medium", "Medium"),
            ("high", "High"),
            ("under_attack", "Under Attack"),
        ],
        string="Security Level",
        help="Choose the Cloudflare security profile...",
    )
    development_mode = fields.Selection(
        [("on", "On"), ("off", "Off")],
        string="Development Mode",
        help="Temporarily bypass Cloudflare cache...",
    )
    browser_cache_ttl = fields.Integer(
        string="Browser Cache TTL (seconds)",
        help="Time in seconds. 0 means respect existing headers.",
    )

    @api.model
    # [@ANCHOR: cloudflare:COMM_zone_settings_default_get]
    def default_get(self, fields_list):
        res = super(CloudflareZoneSettingsWizard, self).default_get(fields_list)
        website_id = res.get("website_id")
        if not website_id:
            website_id = self.env["website"].get_current_website().id

        website = self.env["website"].browse(website_id)
        token, zone_id = website._get_cloudflare_credentials()

        if token and zone_id:

            settings = get_zone_settings(token, zone_id)
            if settings:
                for setting in settings:
                    if (
                        setting.get("id") == "security_level"
                        and "security_level" in fields_list
                    ):
                        res["security_level"] = setting.get("value")
                    elif (
                        setting.get("id") == "development_mode"
                        and "development_mode" in fields_list
                    ):
                        res["development_mode"] = setting.get("value")
                    elif (
                        setting.get("id") == "browser_cache_ttl"
                        and "browser_cache_ttl" in fields_list
                    ):
                        res["browser_cache_ttl"] = setting.get("value")

        return res

    # [@ANCHOR: cloudflare:COMM_zone_settings_action_apply]
    def action_apply_settings(self):
        self.ensure_one()
        token, zone_id = self.website_id._get_cloudflare_credentials()
        if not token or not zone_id:
            raise UserError(
                _("Missing Cloudflare API Token or Zone ID for the selected website.")
            )

        errors = []
        if self.security_level:
            success, msg = update_zone_setting(
                "security_level", self.security_level, token, zone_id
            )
            if not success:
                errors.append(f"Security Level: {msg}")

        if self.development_mode:
            success, msg = update_zone_setting(
                "development_mode", self.development_mode, token, zone_id
            )
            if not success:
                errors.append(f"Development Mode: {msg}")

        # Bug fix (bug-hunt, review_tier 1, 2026-09-09): `is not False` was a
        # vacuous/dead check (bug class 1) -- Odoo's Integer field never
        # caches or returns Python `False`, only a real int with 0 as its own
        # falsy value (odoo/orm/fields_numeric.py: `falsy_value = 0`,
        # `convert_to_cache` always does `int(value or 0)`). So this branch
        # was unconditionally True on every call, and every "Apply" click
        # force-set browser_cache_ttl to 0 ("respect existing headers")
        # whenever `default_get` hadn't pre-populated a live value (missing
        # token/zone, a failed `get_zone_settings` call, or the setting id
        # simply absent from Cloudflare's response) -- silently discarding
        # whatever TTL was actually configured, even when the admin only
        # meant to change security_level or development_mode. Switched to a
        # truthy check, matching security_level/development_mode's own
        # convention just above: this only applies browser_cache_ttl when
        # non-zero. Trade-off: this wizard can no longer be used to
        # explicitly (re-)set the TTL to literal 0 -- a dedicated tri-state
        # (e.g. a separate "apply TTL" checkbox) would be needed to support
        # that without ambiguity; left as a follow-up, not done here.
        if self.browser_cache_ttl:
            success, msg = update_zone_setting(
                "browser_cache_ttl", self.browser_cache_ttl, token, zone_id
            )
            if not success:
                errors.append(f"Browser Cache TTL: {msg}")

        if errors:
            raise UserError(
                _("Failed to update some settings:\n%s") % "\n".join(errors)
            )

        return {
            "type": "ir.actions.client",
            "tag": "display_notification",
            "params": {
                "title": _("Success"),
                "message": _("Zone settings updated successfully."),
                "type": "success",
                "sticky": False,
                "next": {"type": "ir.actions.act_window_close"},
            },
        }
