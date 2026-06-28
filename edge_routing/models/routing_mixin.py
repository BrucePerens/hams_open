# -*- coding: utf-8 -*-
from odoo import models, fields, api, _
from odoo.exceptions import ValidationError
from odoo.addons.edge_routing.utils import slugify, RESERVED_SLUGS
from odoo.addons.distributed_redis_cache.redis_cache import (
    distributed_cache,
    notify_model_invalidation,
)

import json

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

    _website_slug_unique = models.Constraint(
        "UNIQUE(website_slug)", "The Website Slug must be unique!"
    )

    _website_slug_format = models.Constraint(
        r"CHECK(website_slug IS NULL OR website_slug = '' OR website_slug ~ '^[a-z0-9\-]+$')",
        "The Website Slug can only contain lowercase letters, numbers, and hyphens.",
    )

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
        return ["res.users"]

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
            try:
                target_env = self.env["zero_sudo.security.utils"]._get_service_env(
                    "edge_routing.edge_routing_service_account"
                )
            except Exception as e:  # audit-ignore-catch-all
                _logger.warning("Failed to get service env: %s", e)
                target_env = self.env

        record = (
            target_env[self._name]
            .with_context(active_test=False)
            .search([("website_slug", "=ilike", slug)], limit=1)
        )
        return record.id if record else False

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
                vals["website_slug"] = slugify(vals["website_slug"])
            elif vals.get("name"):
                vals["website_slug"] = self._generate_unique_slug(vals["name"])
        return super().create(vals_list)

    # Verified by [@ANCHOR: user_websites:test_slug_cache_invalidation]
    # Verified by [@ANCHOR: user_websites:test_group_slug_cache_invalidation]
    def write(self, vals):
        if "website_slug" in vals:
            if vals.get("website_slug"):
                if len(self) == 1:
                    vals["website_slug"] = self._generate_unique_slug(
                        vals["website_slug"], record_id=self.id
                    )
                else:
                    vals["website_slug"] = slugify(vals["website_slug"])
            else:
                pass  # Clearing the slug is allowed if the field isn't required

        res = super().write(vals)

        if "website_slug" in vals or "name" in vals:
            for record in self:
                if not record.website_slug and record.name:
                    # Auto-generate if missing (and name changed)
                    record.website_slug = self._generate_unique_slug(
                        record.name, record_id=record.id
                    )

                if record.website_slug:
                    payload = json.dumps({"model": self._name})
                    try:
                        self.env["zero_sudo.security.utils"]._notify_cache_invalidation(
                            self._name, record.website_slug
                        )
                        notify_model_invalidation(self.env, self._name)
                        self.env.cr.execute(
                            "SELECT pg_notify(%s, %s)",
                            ("distributed_cache_invalidation", payload),
                        )
                    except Exception as e:  # audit-ignore-catch-all
                        _logger.warning("Failed to notify cache invalidation: %s", e)
        return res

    def unlink(self):
        slugs = self.mapped("website_slug")
        res = super().unlink()
        for slug in slugs:
            if slug:
                payload = json.dumps({"model": self._name})
                try:
                    self.env["zero_sudo.security.utils"]._notify_cache_invalidation(
                        self._name, slug
                    )
                    notify_model_invalidation(self.env, self._name)
                    self.env.cr.execute(
                        "SELECT pg_notify(%s, %s)",
                        ("distributed_cache_invalidation", payload),
                    )
                except Exception as e:  # audit-ignore-catch-all
                    _logger.warning(
                        "Failed to notify cache invalidation on unlink: %s", e
                    )
        return res
