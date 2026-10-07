# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.

from odoo import _, api, models, fields
from odoo.exceptions import UserError, ValidationError


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
    """One Cloudflare Cache Rule, pushed to the zone's ``http_request_cache_settings`` ruleset.

    Odoo is the single source of truth: ``cloudflare.config.manager.action_push_cache_rules``
    builds the whole ruleset from these rows (bypass rules first, by ``sequence``).
    """

    _name = "cloudflare.cache.rule"
    _description = "Cloudflare Cache Rule"
    _order = "sequence, id"

    name = fields.Char(string="Name", required=True)
    sequence = fields.Integer(string="Sequence", default=10)
    active = fields.Boolean(string="Active", default=True)
    rule_action = fields.Selection(
        [
            ("bypass", "Bypass cache"),
            ("cache", "Eligible for cache"),
        ],
        string="Action",
        required=True,
        default="cache",
        help="Bypass: matching requests never use the edge cache. Eligible for cache: matching "
        "requests may be cached, subject to the TTL below and to every active bypass rule.",
    )
    expression = fields.Text(
        string="Expression",
        required=True,
        default="true",
        help='Cloudflare Ruleset Engine expression, for example (http.cookie contains "session_id"). '
        "Use true to match every request.",
    )

    _check_name_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(name)) > 0)", "Name cannot be empty."
    )
    _check_expression_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(expression)) > 0)", "The expression cannot be empty."
    )
    edge_cache_ttl = fields.Integer(
        string="Edge Cache TTL (seconds)",
        help="0 means defer to the origin: the Cloudflare-CDN-Cache-Control header our server sends "
        "(only on pages Odoo's own page cache accepts) decides. A positive value overrides the origin. "
        "Ignored for a bypass rule.",
    )
    bypass_rules = fields.Text(
        string="Notes (legacy)",
        help="Free-text notes only. Not pushed to Cloudflare; use Expression.",
    )
    website_id = fields.Many2one(
        "website",
        string="Website",
        help="Empty applies the rule to every website's zone.",
    )

    def action_push_to_cloudflare(self):
        """Push every active cache rule of the current website to Cloudflare (Odoo is the source)."""
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_waf"
        )
        website = self.env["website"].get_current_website()
        success, msg = (
            self.env["cloudflare.config.manager"]
            .with_user(svc_uid)
            .action_push_cache_rules(website_id=website.id)
        )
        if not success:
            raise UserError(_("Failed to push cache rules: %s") % msg)
        return {
            "type": "ir.actions.client",
            "tag": "display_notification",
            "params": {
                "title": _("Success"),
                "message": msg,
                "type": "success",
                "sticky": False,
            },
        }

    @api.constrains("edge_cache_ttl")
    def _check_edge_cache_ttl(self):
        for rec in self:
            if rec.edge_cache_ttl < 0:
                raise ValidationError(_("The edge cache TTL cannot be negative."))


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
