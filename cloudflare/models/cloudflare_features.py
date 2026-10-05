# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.

from odoo import models, fields, _


class CloudflareZoneSettings(models.Model):
    _name = "cloudflare.zone.settings"
    _description = "Cloudflare Zone Settings"

    name = fields.Char(string="Name", required=True)
    ssl_mode = fields.Selection(
        [
            ("off", "Off"),
            ("flexible", "Flexible"),
            ("full", "Full"),
            ("strict", "Full (strict)"),
        ],
        string="SSL Mode",
    )
    auto_minify = fields.Boolean(string="Auto Minify", help="Automatically minify HTML, CSS, and JS.")
    bot_fight_mode = fields.Boolean(string="Bot Fight Mode", help="Challenge bad bots.")
    website_id = fields.Many2one("website", string="Website")

    _check_name_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(name)) > 0)", "Name cannot be empty."
    )


class CloudflareRateLimit(models.Model):
    _name = "cloudflare.rate.limit"
    _description = "Cloudflare Rate Limit"

    name = fields.Char(string="Name", required=True)

    _check_name_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(name)) > 0)", "Name cannot be empty."
    )
    match_criteria = fields.Char(string="Matching Criteria")
    mitigation_action = fields.Selection(
        [
            ("block", "Block"),
            ("challenge", "Challenge"),
            ("js_challenge", "JS Challenge"),
            ("managed_challenge", "Managed Challenge"),
        ],
        string="Mitigation Action",
    )
    website_id = fields.Many2one("website", string="Website")


class CloudflareCacheRule(models.Model):
    _name = "cloudflare.cache.rule"
    _description = "Cloudflare Cache Rule"
    _order = "sequence, id"

    name = fields.Char(string="Name", required=True)

    _check_name_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(name)) > 0)", "Name cannot be empty."
    )
    edge_cache_ttl = fields.Integer(
        string="Edge Cache TTL (seconds)",
        help="0 on a cache rule means: defer to the origin's Cloudflare-CDN-Cache-Control "
        "header. A positive value overrides the origin. Ignored on a bypass rule.",
    )
    bypass_rules = fields.Text(string="Bypass Rules")
    sequence = fields.Integer(string="Sequence", default=10)
    active = fields.Boolean(string="Active", default=True)
    action = fields.Selection(
        [("bypass", "Bypass cache"), ("cache", "Cache")],
        string="Action",
        required=True,
        default="cache",
        help="Bypass rules are always pushed ahead of cache rules, whatever their "
        "sequence, so pages are never cacheable without the bypass in front of them.",
    )
    expression = fields.Text(
        string="Expression",
        default="true",
        help="Cloudflare Ruleset Engine expression, e.g. (http.cookie contains \"session_id\"). "
        "'true' matches every request.",
    )
    website_id = fields.Many2one("website", string="Website")

    def action_push_to_cloudflare(self):
        """Push every active cache rule of this rule's website to Cloudflare."""
        self.ensure_one()
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_waf"
        )
        website_id = self.website_id.id or self.env["website"].get_current_website().id
        success, msg = (
            self.env["cloudflare.config.manager"]
            .with_user(svc_uid)
            .action_push_cache_rules(website_id=website_id)
        )
        return {
            "type": "ir.actions.client",
            "tag": "display_notification",
            "params": {
                "title": _("Success") if success else _("Cloudflare push failed"),
                "message": msg,
                "type": "success" if success else "danger",
                "sticky": not success,
            },
        }


class CloudflareZeroTrustPolicy(models.Model):
    _name = "cloudflare.zero.trust.policy"
    _description = "Cloudflare Zero Trust Policy"

    name = fields.Char(string="Name", required=True)

    _check_name_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(name)) > 0)", "Name cannot be empty."
    )
    policy_action = fields.Selection(
        [("allow", "Allow"), ("block", "Block"), ("bypass", "Bypass")], string="Action"
    )
    idps = fields.Char(string="Identity Providers (IdPs)")
    website_id = fields.Many2one("website", string="Website")
