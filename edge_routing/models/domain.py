# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

from odoo import models, fields, api, exceptions, _
from odoo.tools import config
from odoo.addons.edge_routing.utils import RESERVED_SLUGS
from odoo.addons.distributed_redis_cache.redis_cache import (
    distributed_cache,
)
import logging
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

    # [@ANCHOR: edge_routing:COMM_domain_check_name]
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

    # [@ANCHOR: edge_routing:COMM_domain_push_pagerduty]
    @api.model
    def push_all_to_pager_duty(self):
        """
        Pushes the full domain routing table to PagerDuty.
        Designed to be executed asynchronously via ir.cron.
        """
        try:
            # Resolve service user securely
            if self.env.registry.loaded:
                self.env.cr.execute("SELECT 1 FROM ir_model_data WHERE module=%s AND name=%s", ('edge_routing', 'edge_routing_service_account'))  # Tested by [@ANCHOR: test_edge_routing_service_account_sql_check]
                if self.env.cr.fetchone():
                    svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                        "edge_routing.edge_routing_service_account"
                    )
                    env_svc = self.with_user(svc_uid).env
                else:
                    env_svc = self.env
            else:
                env_svc = self.env

            all_domains = []
            last_id = 0
            while True:
                batch = env_svc["edge.routing.domain"].search([("id", ">", last_id)], limit=1000, order="id ASC")
                if not batch:
                    break
                all_domains.extend(batch.mapped("name"))
                last_id = batch[-1].id

            if "ham.dns.zone" in env_svc:
                try:
                    # bug-hunt (2026-09-09): savepoint is load-bearing, not
                    # decorative -- see the identical note on
                    # get_target_slug_by_domain below. _get_service_env()'s
                    # own SQL-backed uid lookup can raise a real Postgres
                    # `RAISE EXCEPTION`, which aborts the transaction; the
                    # outer `except Exception` here already recovers this
                    # specific function's own control flow either way (no
                    # further DB call follows in this function today), but
                    # without the savepoint the transaction stays poisoned
                    # for whatever runs next in the SAME transaction --
                    # e.g. ir.cron's own end-of-job bookkeeping writes, if
                    # this method is invoked as part of a larger cron
                    # transaction rather than getting its own.
                    with self.env.cr.savepoint():
                        dns_env_svc = env_svc["zero_sudo.security.utils"]._get_service_env("ham_dns.user_dns_api_service")
                        last_id = 0
                        while True:
                            dns_batch = dns_env_svc["ham.dns.zone"].search([("id", ">", last_id)], limit=1000, order="id ASC")
                            if not dns_batch:
                                break
                            all_domains.extend(dns_batch.mapped("name"))
                            last_id = dns_batch[-1].id
                except Exception as e:  # audit-ignore-catch-all
                    # bug-hunt (2026-09-09): this used to catch only
                    # (KeyError, ValueError) -- but _get_service_env() ->
                    # _get_service_uid() raises AccessError on a bad/missing
                    # xml_id, and the underlying SQL-backed uid resolution
                    # can raise a psycopg2 error, neither of which is a
                    # KeyError/ValueError. A real DNS-zone lookup failure of
                    # either kind would have propagated uncaught out of this
                    # explicitly-labeled "soft dependency" block, crashing
                    # the whole cron run instead of just skipping the DNS
                    # domains. Class 20 (try/except narrower than the real
                    # failure mode).
                    _logger.warning("Soft dependency ham.dns.zone failed: %s", e)

            unique_domains = list(set(all_domains))
            # Send to the API
            host = config.get('odoo_host') or 'odoo'
            response = requests.post(
                f"http://{host}:8069/api/v1/pager_duty/update_domains",
                json={
                    "jsonrpc": "2.0",
                    "method": "call",
                    "params": {"domains": unique_domains},
                },
                timeout=5,
            )
            response.raise_for_status()
        except Exception as e:  # audit-ignore-catch-all
            # bug-hunt (2026-09-09): requests.post()/raise_for_status() raise
            # requests.exceptions.* (ConnectionError, Timeout, HTTPError) on
            # any real network/HTTP failure -- none of those are KeyError or
            # ValueError, so a genuinely offline PagerDuty endpoint used to
            # crash this cron job with an uncaught exception instead of
            # logging and returning, defeating the whole point of wrapping
            # an external HTTP call in a broad "don't fail the cron" guard.
            # Class 20 (try/except narrower than the real failure mode).
            _logger.warning("Failed to sync domains to PagerDuty: %s", e)

    # [@ANCHOR: edge_routing:COMM_domain_crud_cycle]
    def _invalidate_cache(self, names):
        valid_names = [n for n in names if n]
        if valid_names:
            try:
                self.env["zero_sudo.security.utils"]._notify_cache_invalidation(
                    self._name, valid_names
                )
            except Exception as e:  # audit-ignore-catch-all
                # bug-hunt (2026-09-09): _notify_cache_invalidation() issues
                # a raw `self.env.cr.execute("SELECT pg_notify(...)")` --
                # any DB/connection-level failure there raises a psycopg2
                # error, not KeyError/ValueError. A transient DB hiccup
                # during cache-invalidation notification would have
                # propagated uncaught out of create()/write()/unlink(),
                # aborting the whole record mutation over what should be a
                # best-effort cache ping. Class 20.
                _logger.warning("Failed to notify cache invalidation: %s", e)

        try:
            # Trigger cron to run asynchronously, avoiding thread exhaustion and batching O(N) fetches
            cron = self.env.ref('edge_routing.ir_cron_push_pager_duty', raise_if_not_found=False)
            if cron:
                cron._trigger()
        except Exception as e:  # audit-ignore-catch-all
            # bug-hunt (2026-09-09): broadened alongside the cache-invalidation
            # guard above -- cron._trigger() is a DB write (ir.cron.trigger)
            # that can fail for reasons beyond KeyError/ValueError; this path
            # is explicitly "best effort, don't block the CRUD op." Class 20.
            _logger.warning("Failed to trigger PagerDuty sync cron: %s", e)

    # [@ANCHOR: edge_routing:COMM_domain_create]
    @api.model_create_multi
    def create(self, vals_list):
        for vals in vals_list:
            if vals.get("name"):
                vals["name"] = vals["name"].lower().strip()

        records = super(EdgeRoutingDomain, self).create(vals_list)
        self._invalidate_cache([r.name for r in records])
        return records

    # [@ANCHOR: edge_routing:COMM_domain_write]
    def write(self, vals):
        if "name" in vals and vals["name"]:
            vals["name"] = vals["name"].lower().strip()

        old_names = [r.name for r in self]
        res = super(EdgeRoutingDomain, self).write(vals)

        self._invalidate_cache(old_names + [r.name for r in self])
        return res

    # [@ANCHOR: edge_routing:COMM_domain_unlink]
    def unlink(self):
        names = [r.name for r in self]
        res = super(EdgeRoutingDomain, self).unlink()
        self._invalidate_cache(names)
        return res

    # [@ANCHOR: edge_routing:COMM_domain_get_target_slug_by_domain]
    @api.model
    @distributed_cache()
    def get_target_slug_by_domain(self, domain, override_svc_uid=None):
        """
        High-performance RAM cache for domain to slug resolution.
        """
        if not domain:
            return False
        domain = str(domain).lower().strip()

        if override_svc_uid:
            target_env = self.with_user(override_svc_uid).env
        else:
            if self.env.registry.loaded:
                self.env.cr.execute("SELECT 1 FROM ir_model_data WHERE module=%s AND name=%s", ('edge_routing', 'edge_routing_service_account'))  # Tested by [@ANCHOR: test_edge_routing_service_account_sql_check]
                if self.env.cr.fetchone():
                    try:
                        # bug-hunt (2026-09-09): the savepoint is load-
                        # bearing, not decorative -- _get_service_uid()'s
                        # own SQL-backed uid lookup does a real Postgres
                        # `RAISE EXCEPTION` (see
                        # zero_sudo_get_service_uid() in
                        # zero_sudo/data/postgres_procedures.xml) on a
                        # missing/disabled/non-service account. A raw SQL
                        # RAISE EXCEPTION aborts the CURRENT transaction --
                        # without a savepoint to roll back to, the
                        # `target_env = self.env` fallback below would
                        # still be caught here, but the very next line's
                        # `target_env[self._name].search(...)` would then
                        # raise `InFailedSqlTransaction` uncaught, since
                        # every statement on a poisoned transaction fails
                        # until it's rolled back to a savepoint (or the
                        # whole transaction). Class 20, one level deeper.
                        with self.env.cr.savepoint():
                            target_env = self.env["zero_sudo.security.utils"]._get_service_env(
                                "edge_routing.edge_routing_service_account"
                            )
                    except Exception:  # audit-ignore-catch-all
                        # bug-hunt (2026-09-09): _get_service_env() raises
                        # AccessError (not KeyError/ValueError) on a bad
                        # xml_id, plus a possible psycopg2 error from the
                        # SQL-backed uid lookup -- this fallback-to-self
                        # path only actually triggered for a KeyError/
                        # ValueError, so a real service-account resolution
                        # failure would have crashed domain resolution
                        # instead of degrading to self.env. Class 20.
                        _logger.warning("Failed to get service env")
                        target_env = self.env
                else:
                    target_env = self.env
            else:
                target_env = self.env

        record = (
            target_env[self._name]
            .with_context(active_test=False)
            .search([("name", "=", domain)], limit=1)
        )
        return record.target_slug if record else False
