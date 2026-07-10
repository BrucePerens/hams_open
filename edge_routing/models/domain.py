# -*- coding: utf-8 -*-
from odoo import models, fields, api, exceptions, _
from odoo.addons.edge_routing.utils import RESERVED_SLUGS
from odoo.addons.distributed_redis_cache.redis_cache import (
    distributed_cache,
    notify_model_invalidation,
)
import logging
import json
import requests

_logger = logging.getLogger(__name__)


class EdgeRoutingDomain(models.Model):
    _name = "edge.routing.domain"
    _description = "Custom Domain Mapping"

    name = fields.Char("Custom Domain", required=True, copy=False, help="e.g. www.myclub.org")
    target_slug = fields.Char(
        "Target Slug", required=True, help="The website_slug this domain maps to"
    )

    _name_uniq = models.Constraint("UNIQUE(name)", "This domain is already mapped!")

    @api.constrains("name", "target_slug")
    def _check_name(self):
        for record in self:
            if not record.name or "." not in record.name:
                raise exceptions.ValidationError(
                    _("Domain must be a valid FQDN (e.g. www.myclub.org)")
                )
            if record.target_slug and record.target_slug.lower() in RESERVED_SLUGS:
                raise exceptions.ValidationError(
                    _("This target slug is reserved and cannot be used.")
                )

    @api.model
    def push_all_to_pager_duty(self):
        """
        Pushes the full domain routing table to PagerDuty.
        Designed to be executed asynchronously via ir.cron.
        """
        try:
            # Resolve service user securely
            svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                "edge_routing.edge_routing_service_account"
            )
            env_svc = self.with_user(svc_uid).env

            all_domains = []
            offset = 0
            while True:
                batch = env_svc["edge.routing.domain"].search([], offset=offset, limit=1000).mapped("name")
                if not batch:
                    break
                all_domains.extend(batch)
                offset += 1000

            if "ham.dns.zone" in env_svc:
                try:
                    dns_env_svc = env_svc["zero_sudo.security.utils"]._get_service_env("ham_dns.user_dns_api_service")
                    offset = 0
                    while True:
                        dns_domains = dns_env_svc["ham.dns.zone"].search([], offset=offset, limit=1000).mapped("name")
                        if not dns_domains:
                            break
                        all_domains.extend(dns_domains)
                        offset += 1000
                except Exception as e:  # audit-ignore-catch-all
                    _logger.warning("Soft dependency ham.dns.zone failed: %s", e)

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

    def _invalidate_cache(self, names):
        if names:
            payload = json.dumps({"model": self._name})
            for name in names:
                if name:
                    try:
                        self.env["zero_sudo.security.utils"]._notify_cache_invalidation(
                            self._name, name
                        )
                    except Exception:  # audit-ignore-catch-all
                        _logger.warning("Failed to invalidate cache for domain %s", name)
            try:
                notify_model_invalidation(self.env, self._name)
                self.env.cr.execute(
                    "SELECT pg_notify(%s, %s)",
                    ("distributed_cache_invalidation", payload),
                )
            except Exception as e:  # audit-ignore-catch-all
                _logger.warning("Failed to notify global cache invalidation: %s", e)

        try:
            # Trigger cron to run asynchronously, avoiding thread exhaustion and batching O(N) fetches
            cron = self.env.ref('edge_routing.ir_cron_push_pager_duty', raise_if_not_found=False)
            if cron:
                cron._trigger()
        except Exception as e:  # audit-ignore-catch-all
            _logger.warning("Failed to trigger PagerDuty sync cron: %s", e)

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

        if self.env.registry.loaded:
            try:
                target_env = self.env["zero_sudo.security.utils"]._get_service_env(
                    "edge_routing.edge_routing_service_account"
                )
            except Exception:  # audit-ignore-catch-all
                _logger.warning("Failed to get service env")
                target_env = self.env
        else:
            target_env = self.env

        record = (
            target_env[self._name]
            .with_context(active_test=False)
            .search([("name", "=", domain)], limit=1)
        )
        return record.target_slug if record else False
