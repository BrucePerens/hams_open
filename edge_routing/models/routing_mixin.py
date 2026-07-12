# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

from odoo import models, fields, api, _
from odoo.exceptions import ValidationError, UserError
from odoo.addons.edge_routing.utils import slugify, RESERVED_SLUGS
from odoo.addons.distributed_redis_cache.redis_cache import (
    distributed_cache,
)

from psycopg2 import sql as psql

import logging

_logger = logging.getLogger(__name__)


class EdgeRoutingMixin(models.AbstractModel):
    _name = "edge.routing.mixin"
    _description = "Edge Routing Mixin"
    name = fields.Char(string="Name", default=lambda self: self._description)

    website_slug = fields.Char(
        string="Website Slug",
        index="trigram",
        help="The URL-friendly identifier for the site. Alphanumeric and hyphens only.",
    )

    _website_slug_unique = models.Constraint("EXCLUDE (website_slug WITH =) WHERE (website_slug != '')", "The Website Slug must be unique!")

    _website_slug_format = models.Constraint("CHECK(website_slug IS NULL OR website_slug = '' OR website_slug ~ '^[a-z0-9\\-]+$')", 'The Website Slug can only contain lowercase letters, numbers, and hyphens.')

    @api.constrains("website_slug")
    def _check_reserved_slugs(self):
        for record in self:
            if record.website_slug and record.website_slug in RESERVED_SLUGS:
                raise ValidationError(
                    _("The slug '%s' is reserved and cannot be used.")
                )

    @api.model
    def _get_routing_models(self):
        """Returns the list of models that share the global vanity URL namespace."""
        models = []
        for model_name, model_class in self.env.registry.items():
            if issubclass(model_class, type(self.env['edge.routing.mixin'])) and model_name != 'edge.routing.mixin':
                models.append(model_name)
        return models

    def _generate_unique_slug(self, base_string, record_id=False, forbidden_slugs=None):
        """
        Generates a URL-safe, globally unique slug. Cross-references reserved routes
        and all models that implement edge.routing.mixin.
        """
        if not base_string:
            return ""

        base_slug = slugify(base_string)
        
        routing_model_names = self._get_routing_models()
        existing_slugs = set(RESERVED_SLUGS)
        if forbidden_slugs:
            existing_slugs.update(forbidden_slugs)

        if self.env.registry.loaded:
            self.env.cr.execute("SELECT 1 FROM ir_model_data WHERE module=%s AND name=%s", ('edge_routing', 'edge_routing_service_account')) # audit-ignore-sql: Tested by [@ANCHOR: test_edge_routing_service_account_sql_check]
            if self.env.cr.fetchone():
                try:
                    with self.env.cr.savepoint():
                        env_svc = self.env["zero_sudo.security.utils"]._get_service_env(
                            "edge_routing.edge_routing_service_account"
                        )
                except Exception as e:  # audit-ignore-catch-all
                    _logger.warning("Error: %s", e)
                    env_svc = self.env
            else:
                env_svc = self.env
        else:
            env_svc = self.env

        for model_name in routing_model_names:
            if model_name not in self.env:
                continue

            domain = [("website_slug", "=like", f"{base_slug}%")]
            if record_id and model_name == self._name:
                domain.append(("id", "!=", record_id))

            try:
                env_target = env_svc[model_name]
            except Exception as e:  # audit-ignore-catch-all
                _logger.warning("Error: %s", e)
                env_target = self.env[model_name]

            slugs = env_target.with_context(active_test=False).search_read(domain, ["website_slug"])
            existing_slugs.update(s["website_slug"] for s in slugs if s.get("website_slug"))

        slug = base_slug
        counter = 1
        max_retries = 1000

        while True:
            if counter > max_retries:
                raise ValidationError(
                    _("Unable to generate a unique website slug after %s attempts.")
                    % max_retries
                )

            if slug not in existing_slugs:
                lock_hash = self.env[
                    "zero_sudo.security.utils"
                ]._get_deterministic_hash(slug)
                self.env.cr.execute(
                    "SELECT pg_try_advisory_xact_lock(%s)", (lock_hash,)
                )
                lock_acquired = self.env.cr.fetchone()[0]
                if lock_acquired:
                    return slug

            slug = f"{base_slug}-{counter}"
            counter += 1

    # Verified by [@ANCHOR: user_websites:test_group_site_routing]
    @api.model
    @distributed_cache()
    def get_record_by_slug(self, slug, override_svc_uid=None):
        """
        High-performance RAM cache for slug resolution.
        Prevents full DB queries on every public profile view.
        """
        if not slug:
            return False
        slug = str(slug).lower()

        if override_svc_uid:
            target_env = self.with_user(override_svc_uid).env
        else:
            if self.env.registry.loaded:
                self.env.cr.execute("SELECT 1 FROM ir_model_data WHERE module=%s AND name=%s", ('edge_routing', 'edge_routing_service_account')) # audit-ignore-sql: Tested by [@ANCHOR: test_edge_routing_service_account_sql_check]
                if self.env.cr.fetchone():
                    try:
                        with self.env.cr.savepoint():
                            target_env = self.env["zero_sudo.security.utils"]._get_service_env(
                                "edge_routing.edge_routing_service_account"
                            )
                    except Exception as e:  # audit-ignore-catch-all
                        _logger.warning("Failed to get service env: %s", e)
                        target_env = self.env
                else:
                    target_env = self.env
            else:
                target_env = self.env

        record = (
            target_env[self._name]
            .with_context(active_test=False)
            .search([("website_slug", "=ilike", slug)], limit=1)
        )
        return record.id if record else False

    @api.model
    def get_record_by_domain(self, domain, override_svc_uid=None):
        """
        Helper to map a custom domain directly to a record ID.
        Uses the edge.routing.domain distributed cache to resolve the slug,
        and then uses get_record_by_slug to resolve the record.
        """
        if not domain:
            return False

        slug = self.env["edge.routing.domain"].get_target_slug_by_domain(domain)
        if not slug:
            return False

        return self.get_record_by_slug(slug, override_svc_uid=override_svc_uid)

    @api.model_create_multi
    def create(self, vals_list):
        assigned_slugs = set()
        for vals in vals_list:
            if vals.get("website_slug"):
                slug_to_check = slugify(vals["website_slug"])
                if slug_to_check in RESERVED_SLUGS:
                    raise ValidationError(_("The slug '%s' is reserved and cannot be used.") % vals["website_slug"])
                vals["website_slug"] = self._generate_unique_slug(vals["website_slug"], forbidden_slugs=assigned_slugs)
                assigned_slugs.add(vals["website_slug"])
            elif vals.get("name"):
                vals["website_slug"] = self._generate_unique_slug(vals["name"], forbidden_slugs=assigned_slugs)
                assigned_slugs.add(vals["website_slug"])
        return super().create(vals_list)

    # Verified by [@ANCHOR: user_websites:test_slug_cache_invalidation]
    # Verified by [@ANCHOR: user_websites:test_group_slug_cache_invalidation]
    def write(self, vals):
        if vals.get("website_slug"):
            slug_to_check = slugify(vals["website_slug"])
            if slug_to_check in RESERVED_SLUGS:
                raise ValidationError(_("The slug '%s' is reserved and cannot be used.") % vals["website_slug"])
            
            if len(self) > 1:
                raise UserError(_("Cannot assign the same website slug to multiple records."))
            vals["website_slug"] = self._generate_unique_slug(
                vals["website_slug"], record_id=self.id
            )
        
        if len(self) == 1 and not vals.get("website_slug") and not self.website_slug and "name" in vals:
            vals["website_slug"] = self._generate_unique_slug(vals["name"], record_id=self.id)

        old_slugs = [s for s in self.mapped("website_slug") if s]
        
        res = super().write(vals)

        if "website_slug" in vals or "name" in vals:
            new_slugs = [s for s in self.mapped("website_slug") if s]
            all_slugs_to_invalidate = list(set(old_slugs + new_slugs))
            
            if all_slugs_to_invalidate:
                try:
                    self.env["zero_sudo.security.utils"]._notify_cache_invalidation(
                        self._name, all_slugs_to_invalidate
                    )
                except Exception as e:  # audit-ignore-catch-all
                    _logger.warning("Failed to notify cache invalidation for %s: %s", all_slugs_to_invalidate, e)
                    
            # Handle the batch write edge case for missing slugs when name is updated
            if "name" in vals and len(self) > 1:
                records_to_update = self.filtered(lambda r: not r.website_slug and "name" in r._fields and r.name)
                if records_to_update:
                    updates = []
                    assigned_slugs = set(all_slugs_to_invalidate)
                    for record in records_to_update:
                        new_slug = self._generate_unique_slug(
                            record.name, record_id=record.id, forbidden_slugs=assigned_slugs
                        )
                        assigned_slugs.add(new_slug)
                        updates.append((record.id, new_slug))
                        
                    if updates:
                        values_sql = psql.SQL(', ').join(psql.SQL('(%s, %s)') for _ in updates)
                        query = psql.SQL("UPDATE {} AS t SET website_slug = c.website_slug FROM (VALUES {}) AS c(id, website_slug) WHERE t.id = c.id").format(
                            psql.Identifier(self._table),
                            values_sql
                        )
                        params = []
                        for rid, slug in updates:
                            params.extend([rid, slug])
                        self.env.cr.execute(query, params)
                        records_to_update.invalidate_recordset(['website_slug'])

        return res

    def unlink(self):
        slugs = [s for s in self.mapped("website_slug") if s]
        res = super().unlink()
        if slugs:
            try:
                self.env["zero_sudo.security.utils"]._notify_cache_invalidation(
                    self._name, slugs
                )
            except Exception as e:  # audit-ignore-catch-all
                _logger.warning("Failed to notify local cache invalidation on unlink for %s: %s", slugs, e)
        return res
