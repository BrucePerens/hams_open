# -*- coding: utf-8 -*-
from odoo import models, fields, api, _
from odoo.exceptions import ValidationError, UserError
from odoo.addons.edge_routing.utils import slugify, RESERVED_SLUGS
from odoo.addons.distributed_redis_cache.redis_cache import (
    distributed_cache,
    notify_model_invalidation,
)

import json
from psycopg2 import sql as psql

import logging

_logger = logging.getLogger(__name__)


class EdgeRoutingMixin(models.AbstractModel):
    _name = "edge.routing.mixin"
    _description = "Edge Routing Mixin"

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

    @api.model
    def _check_slug_collision(self, env_target, domain):
        return bool(env_target.with_context(active_test=False).search(domain, limit=1))

    def _generate_unique_slug(self, base_string, record_id=False):
        """
        Generates a URL-safe, globally unique slug. Cross-references reserved routes
        and all models that implement edge.routing.mixin.
        """
        if not base_string:
            return ""

        base_slug = slugify(base_string)
        slug = base_slug
        counter = 1
        max_retries = 1000

        routing_model_names = self._get_routing_models()

        while True:
            if counter > max_retries:
                raise ValidationError(
                    _("Unable to generate a unique website slug after %s attempts.")
                    % max_retries
                )

            if slug in RESERVED_SLUGS:
                slug = f"{base_slug}-{counter}"
                counter += 1
                continue

            collision = False
            for model_name in routing_model_names:
                if model_name not in self.env:
                    continue

                domain = [("website_slug", "=", slug)]
                if record_id and model_name == self._name:
                    domain.append(("id", "!=", record_id))

                if self.env.registry.loaded:
                    try:
                        with self.env.cr.savepoint():
                            env_svc = self.env["zero_sudo.security.utils"]._get_service_env(
                                "edge_routing.edge_routing_service_account"
                            )
                            env_target = env_svc[model_name]
                    except Exception as e:  # audit-ignore-catch-all
                        _logger.warning(
                            "Failed to get service env for %s: %s", model_name, e
                        )
                        env_target = self.env[model_name]
                else:
                    env_target = self.env[model_name]

                if self._check_slug_collision(env_target, domain):
                    collision = True
                    break

            if not collision:
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
                try:
                    target_env = self.env["zero_sudo.security.utils"]._get_service_env(
                        "edge_routing.edge_routing_service_account"
                    )
                except Exception as e:  # audit-ignore-catch-all
                    _logger.warning("Failed to get service env: %s", e)
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
        for vals in vals_list:
            if vals.get("website_slug"):
                slug_to_check = slugify(vals["website_slug"])
                if slug_to_check in RESERVED_SLUGS:
                    raise ValidationError(_("The slug '%s' is reserved and cannot be used.") % vals["website_slug"])
                vals["website_slug"] = self._generate_unique_slug(vals["website_slug"])
            elif vals.get("name"):
                vals["website_slug"] = self._generate_unique_slug(vals["name"])
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
            all_slugs_to_invalidate = set(old_slugs + new_slugs)
            
            if all_slugs_to_invalidate:
                payload = json.dumps({"model": self._name})
                for slug in all_slugs_to_invalidate:
                    try:
                        self.env["zero_sudo.security.utils"]._notify_cache_invalidation(
                            self._name, slug
                        )
                    except Exception as e:  # audit-ignore-catch-all
                        _logger.warning("Failed to notify cache invalidation for %s: %s", slug, e)
                try:
                    notify_model_invalidation(self.env, self._name)
                    self.env.cr.execute(
                        "SELECT pg_notify(%s, %s)",
                        ("distributed_cache_invalidation", payload),
                    )
                except Exception as e:  # audit-ignore-catch-all
                    _logger.warning("Failed to notify global cache invalidation: %s", e)
                    
            # Handle the batch write edge case for missing slugs when name is updated
            if "name" in vals and len(self) > 1:
                for record in self:
                    if not record.website_slug and "name" in record._fields and record.name:
                        new_slug = self._generate_unique_slug(
                            record.name, record_id=record.id
                        )
                        query = psql.SQL("UPDATE {} SET website_slug = %s WHERE id = %s").format(
                            psql.Identifier(self._table)
                        )
                        self.env.cr.execute(
                            query,
                            (new_slug, record.id)
                        )
                        record.invalidate_recordset(['website_slug'])

        return res

    def unlink(self):
        slugs = self.mapped("website_slug")
        res = super().unlink()
        if slugs:
            payload = json.dumps({"model": self._name})
            for slug in slugs:
                if slug:
                    try:
                        self.env["zero_sudo.security.utils"]._notify_cache_invalidation(
                            self._name, slug
                        )
                    except Exception as e:  # audit-ignore-catch-all
                        _logger.warning("Failed to notify local cache invalidation on unlink for %s: %s", slug, e)
            try:
                notify_model_invalidation(self.env, self._name)
                self.env.cr.execute(
                    "SELECT pg_notify(%s, %s)",
                    ("distributed_cache_invalidation", payload),
                )
            except Exception as e:  # audit-ignore-catch-all
                _logger.warning(
                    "Failed to notify global cache invalidation on unlink: %s", e
                )
        return res
