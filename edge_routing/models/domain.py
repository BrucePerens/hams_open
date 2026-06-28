# -*- coding: utf-8 -*-
from odoo import models, fields, api, exceptions, _
from odoo.addons.edge_routing.utils import RESERVED_SLUGS
from odoo.addons.distributed_redis_cache.redis_cache import (
    distributed_cache,
    notify_model_invalidation,
)
import odoo
import logging
import json
import requests

_logger = logging.getLogger(__name__)


class EdgeRoutingDomain(models.Model):
    _name = "edge.routing.domain"
    _description = "Custom Domain Mapping"

    name = fields.Char("Custom Domain", required=True, help="e.g. www.myclub.org")
    target_slug = fields.Char(
        "Target Slug", required=True, help="The website_slug this domain maps to"
    )

    _name_uniq = models.Constraint("UNIQUE(name)", "This domain is already mapped!")

    @api.constrains("name")
    def _check_name(self):
        for record in self:
            if not record.name or "." not in record.name:
                raise exceptions.ValidationError(
                    _("Domain must be a valid FQDN (e.g. www.myclub.org)")
                )
            if record.name.lower() in RESERVED_SLUGS:
                raise exceptions.ValidationError(
                    _("This domain name is reserved and cannot be used.")
                )

    def _invalidate_cache(self, names):
        for name in names:
            if name:
                payload = json.dumps({"model": self._name})
                try:
                    self.env["zero_sudo.security.utils"]._notify_cache_invalidation(
                        self._name, name
                    )
                    notify_model_invalidation(self.env, self._name)
                    self.env.cr.execute(
                        "SELECT pg_notify(%s, %s)",
                        ("distributed_cache_invalidation", payload),
                    )
                except Exception:  # audit-ignore-catch-all
                    _logger.warning("Failed to invalidate cache for domain %s", name)

        # Async push to Pager Duty
        def push_to_pager_duty():
            try:
                # Use a new cursor or environment to get all domains
                with odoo.registry(self.env.cr.dbname).cursor() as cr:
                    # Resolve service user instead of SUPERUSER_ID
                    cr.execute(
                        "SELECT id FROM res_users WHERE login = 'sys_provisioner'"
                    )
                    row = cr.fetchone()
                    svc_id = row[0] if row else 2
                    env = odoo.api.Environment(cr, svc_id, {})
                    all_domains = (
                        env["edge.routing.domain"].search([], limit=1000).mapped("name")
                    )

                    if "ham.dns.zone" in env:
                        try:
                            # Soft-dependency on ham_dns to include subdomains
                            dns_env_svc = env[
                                "zero_sudo.security.utils"
                            ]._get_service_env("ham_dns.user_dns_api_service")
                            dns_domains = (
                                dns_env_svc["ham.dns.zone"]
                                .search([], limit=5000)
                                .mapped("name")
                            )
                            all_domains.extend(dns_domains)
                        except Exception as e:  # audit-ignore-catch-all
                            _logger.warning(
                                "Soft dependency ham.dns.zone failed: %s", e
                            )

                    unique_domains = list(set(all_domains))
                    # Send to the API
                    requests.post(
                        "http://odoo:8069/api/v1/pager_duty/update_domains",
                        json={
                            "jsonrpc": "2.0",
                            "method": "call",
                            "params": {"domains": unique_domains},
                        },
                        timeout=5,
                    )
            except Exception as e:  # audit-ignore-catch-all
                _logger.warning("Failed to sync domains to Pager Duty: %s", e)

        try:
            self.env.cr.postcommit.add(push_to_pager_duty)
        except Exception as e:  # audit-ignore-catch-all
            _logger.warning("Failed to add postcommit hook: %s", e)

    @api.model_create_multi
    def create(self, vals_list):
        for vals in vals_list:
            if vals.get("name"):
                vals["name"] = vals["name"].lower().strip()

        records = super(EdgeRoutingDomain, self).create(vals_list)
        self._invalidate_cache([r.name for r in records])
        return records

    def write(self, vals):
        if "name" in vals and vals["name"]:
            vals["name"] = vals["name"].lower().strip()

        old_names = [r.name for r in self]
        res = super(EdgeRoutingDomain, self).write(vals)

        self._invalidate_cache(old_names + [r.name for r in self])
        return res

    def unlink(self):
        names = [r.name for r in self]
        res = super(EdgeRoutingDomain, self).unlink()
        self._invalidate_cache(names)
        return res

    @api.model
    @distributed_cache()
    def get_target_slug_by_domain(self, domain):
        """
        High-performance RAM cache for domain to slug resolution.
        """
        if not domain:
            return False
        domain = str(domain).lower().strip()

        try:
            target_env = self.env["zero_sudo.security.utils"]._get_service_env(
                "edge_routing.edge_routing_service_account"
            )
        except Exception:  # audit-ignore-catch-all
            _logger.warning("Failed to get service env")
            target_env = self.env

        record = (
            target_env[self._name]
            .with_context(active_test=False)
            .search([("name", "=", domain)], limit=1)
        )
        return record.target_slug if record else False
