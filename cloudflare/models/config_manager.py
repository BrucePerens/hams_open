# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0.
import json
import logging
from odoo import models, api, fields
from odoo.exceptions import AccessError
from ..utils.cloudflare_api import (
    get_zone_ruleset,
    update_zone_ruleset,
    create_zone_ruleset,
)

_logger = logging.getLogger(__name__)

DEFAULT_WAF_RULES = [
    {
        "sequence": 10,
        "name": "Block Legacy XML-RPC",
        "action": "block",
        "expression": '(http.request.uri.path contains "/xmlrpc")',
        "description": "SECURITY: Blocks legacy XML-RPC access.",
    },
    {
        "sequence": 20,
        "name": "Protect Database Manager",
        "action": "block",
        "expression": '(http.request.uri.path eq "/odoo/database/manager") or (http.request.uri.path eq "/odoo/database/selector")',
        "description": "SECURITY: Prevents public access to the Odoo database manager interface.",
    },
    {
        "sequence": 30,
        "name": "API Scraper Protection",
        "action": "managed_challenge",
        "expression": '(http.request.uri.path contains "/api/v1/") and not cf.client.bot',
        "description": "PERFORMANCE: Protects headless API routes from aggressive, unverified scrapers.",
    },
]


class CloudflareConfigManager(models.AbstractModel):
    _name = "cloudflare.config.manager"
    _description = "Cloudflare Configuration Manager"
    name = fields.Char(string="Name", default=lambda self: self._description)

    @api.model
    def _trigger_edge_purge_static_assets(self):
        """
        Scans static directories during Odoo boot. If a modification is detected,
        it automatically triggers an edge purge for the 'odoo-static-assets' Cache-Tag.
        """
        max_mtime = 0.0
        utils = self.env["zero_sudo.security.utils"]

        # ADR-0064: The Cloudflare Purge account lacks base.group_user and cannot read ir.module.module.
        # We must use the generalized facility service account for deep framework reads.
        try:
            facility_uid = utils._get_service_uid(
                "zero_sudo.odoo_facility_service_internal"
            )
        except AccessError as e:
            _logger.info(
                "Service accounts not yet available, deferring static mtime check: %s",
                e,
            )
            return

        # ADR-0002: Use mixin to perform memory-based scan
        max_mtime, _ = self.env["caching.mixin"].with_user(facility_uid).get_fs_stats()
        latest_mtime = int(max_mtime)

        try:
            # Use centralized config utilities instead of manual service environments
            last_mtime = int(
                utils._get_system_param("cloudflare.last_static_mtime", "0")
            )

            if latest_mtime > last_mtime:
                # Update the system parameter using the centralized utility
                utils._set_system_param(
                    "cloudflare.last_static_mtime", str(latest_mtime)
                )

                # Trigger the purge using the specific purger environment
                env_purger = utils._get_service_env("cloudflare.user_cloudflare_purge")

                # Multi-Website Purge: Static assets should be purged across all configured websites
                # We use the purger environment to access website credentials securely
                last_id = 0
                while True:
                    websites = env_purger["website"].search(
                        [("id", ">", last_id)], order="id asc", limit=1000
                    )
                    if not websites:
                        break
                    last_id = websites[-1].id
                    for website in websites:
                        token, zone_id = website._get_cloudflare_credentials()
                        if token and zone_id:
                            env_purger["cloudflare.purge.queue"].enqueue_tags(
                                ["odoo-static-assets"], website_id=website.id
                            )

                _logger.info(
                    "[*] Static assets modified (%s > %s). Triggered Cloudflare purge for 'odoo-static-assets' across all websites.",
                    latest_mtime,
                    last_mtime,
                )
        except (ValueError, OSError, RuntimeError, AccessError) as e:
            _logger.exception("Failed to process static mtime purge: %s", e)

    @api.model
    def initialize_cloudflare_state(self):
        _logger.info("[*] Initializing Cloudflare Edge State across Websites...")
        last_id = 0
        while True:
            websites = self.env["website"].search(
                [("id", ">", last_id)], order="id asc", limit=1000
            )
            if not websites:
                break
            last_id = websites[-1].id
            for website in websites:
                token, zone_id = website._get_cloudflare_credentials()
                if not token or not zone_id:
                    continue

                existing_ruleset = get_zone_ruleset(
                    "http_request_firewall_custom", token, zone_id
                )
                if existing_ruleset and existing_ruleset.get("rules"):
                    _logger.info(
                        "[+] Existing rules detected for %s. Backing up and syncing.",
                        website.name,
                    )
                    # ADR-0001: Headless Mutation Context
                    self.env["cloudflare.config.backup"].create(
                        {
                            "name": f"Pre-Odoo Backup ({website.name})",
                            "raw_json": json.dumps(existing_ruleset, indent=4),
                            "website_id": website.id,
                        }
                    )
                    self.action_pull_waf_rules(website_id=website.id)
                    continue

                vals_list = []
                for rule_vals in DEFAULT_WAF_RULES:
                    vals = dict(rule_vals)
                    vals["website_id"] = website.id
                    vals_list.append(vals)

                if vals_list:
                    # ADR-0001: Headless Mutation Context
                    self.env["cloudflare.waf.rule"].create(vals_list)

                self.action_push_waf_rules(website_id=website.id)

    @api.model
    def action_pull_waf_rules(self, website_id=None):
        # [@ANCHOR: cf_action_pull_waf_rules]
        website = (
            self.env["website"].browse(website_id)
            if website_id
            else self.env["website"].get_current_website()
        )
        token, zone_id = website._get_cloudflare_credentials()

        existing_ruleset = get_zone_ruleset(
            "http_request_firewall_custom", token, zone_id
        )
        if not existing_ruleset:
            return (
                False,
                f"No custom firewall ruleset found in Cloudflare for {website.name}.",
            )

        # ADR-0001: Headless Mutation Context
        self.env["cloudflare.waf.rule"].search(
            [("website_id", "=", website.id)], limit=1000
        ).unlink()

        rules = existing_ruleset.get("rules", [])
        vals_list = []
        for i, r in enumerate(rules):
            vals_list.append(
                {
                    "sequence": (i + 1) * 10,
                    "name": r.get(
                        "description", "Imported Rule " + r.get("id", "")
                    ).split(":")[0][:50],
                    "action": r.get("action", "block"),
                    "expression": r.get("expression", ""),
                    "description": r.get("description", ""),
                    "active": r.get("enabled", True),
                    "website_id": website.id,
                }
            )
        if vals_list:
            # ADR-0001: Headless Mutation Context
            self.env["cloudflare.waf.rule"].create(vals_list)
        return True, f"Successfully pulled rules from Cloudflare for {website.name}."

    @api.model
    def action_push_waf_rules(self, website_id=None):
        # [@ANCHOR: cf_action_push_waf_rules]
        website = (
            self.env["website"].browse(website_id)
            if website_id
            else self.env["website"].get_current_website()
        )
        token, zone_id = website._get_cloudflare_credentials()
        if not token or not zone_id:
            return False, f"Missing API credentials for {website.name}."

        odoo_rules = self.env["cloudflare.waf.rule"].search(
            [("website_id", "=", website.id)], limit=1000
        )

        cf_rules_payload = []
        for r in odoo_rules:
            cf_rules_payload.append(
                {
                    "action": r.action,
                    "expression": r.expression,
                    "description": r.description or r.name,
                    "enabled": r.active,
                }
            )

        ruleset_payload = {
            "name": f"Odoo WAF Rules - {website.name}",
            "kind": "zone",
            "phase": "http_request_firewall_custom",
            "rules": cf_rules_payload,
        }

        existing_ruleset = get_zone_ruleset(
            "http_request_firewall_custom", token, zone_id
        )
        if existing_ruleset and "id" in existing_ruleset:
            return update_zone_ruleset(
                existing_ruleset["id"], ruleset_payload, token, zone_id
            )
        else:
            return create_zone_ruleset(ruleset_payload, token, zone_id)
