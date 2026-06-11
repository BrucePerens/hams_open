# -*- coding: utf-8 -*-
"""
This file defines the Odoo model for User Websites Groups.
"""

from odoo import models, fields, api, _
from odoo.exceptions import ValidationError, AccessError
from psycopg2 import IntegrityError
from ..utils import slugify, RESERVED_SLUGS
import json
from odoo.addons.distributed_redis_cache.redis_cache import (
    distributed_cache,
    invalidate_model_cache,
)


class UserWebsitesGroup(models.Model):
    """
    Represents a group of users who can manage a shared website.
    """

    _name = "user.websites.group"
    _description = "User Websites Group"
    _inherit = ["mail.thread", "mail.activity.mixin"]

    # --- Fields Definition ---
    name = fields.Char(string="Group Name", required=True, tracking=True)

    website_slug = fields.Char(
        string="Website Slug",
        index="trigram",
        help="The URL-friendly identifier for the group site. Alphanumeric and hyphens only.",
    )

    odoo_group_id = fields.Many2one(
        "res.groups",
        string="Linked Odoo Group",
        required=True,
        ondelete="cascade",
        help="The Odoo security group associated with this website.",
    )

    member_ids = fields.Many2many(
        "res.users",
        related="odoo_group_id.user_ids",
        string="Group Members",
        readonly=False,
        help="Users who have editing rights for this group site.",
    )

    website_page_ids = fields.One2many(
        "website.page",
        "user_websites_group_id",
        string="Group Pages",
        help="Pages belonging to this group website.",
    )

    blog_post_ids = fields.One2many(
        "blog.post",
        "user_websites_group_id",
        string="Group Blog Posts",
        help="Blog posts belonging to this group.",
    )

    company_id = fields.Many2one(
        "res.company",
        string="Company",
        required=True,
        default=lambda self: self.env.company,
    )

    # --- Odoo 19 Constraint Syntax ---
    _website_slug_unique = models.Constraint(
        "UNIQUE(website_slug)", "The Group Website Slug must be unique!"
    )

    _website_slug_format = models.Constraint(
        r"CHECK(website_slug IS NULL OR website_slug = '' OR website_slug ~ '^[a-z0-9\-]+$')",
        "The Group Website Slug can only contain lowercase letters, numbers, and hyphens.",
    )

    @api.constrains("website_slug")
    def _check_reserved_slugs(self):
        for record in self:
            if record.website_slug and record.website_slug in RESERVED_SLUGS:
                raise ValidationError(
                    _("The slug '%s' is reserved and cannot be used.")
                    % record.website_slug
                )

    @api.model
    @distributed_cache()
    def _get_group_id_by_slug(self, slug, override_svc_uid=None):
        # Tested by [@ANCHOR: user_websites:test_group_site_routing]
        if not slug:
            return False
        svc_uid = override_svc_uid or self.env[
            "zero_sudo.security.utils"
        ]._get_service_uid("zero_sudo.user_websites_service_account")
        group = (
            self.env["user.websites.group"]
            .with_user(svc_uid)
            .search([("website_slug", "=ilike", slug)], limit=1)
        )
        return group.id if group else False

    # --- Slug Generation & Management ---

    @api.model
    def _generate_unique_slug(self, base_string, record_id=False):
        """
        Generates a URL-safe, globally unique slug across groups and users.
        """
        if not base_string:
            return ""

        base_slug = slugify(base_string)
        slug = base_slug
        counter = 1
        max_retries = 1000

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

            group_domain = [("website_slug", "=", slug)]
            if record_id:
                group_domain.append(("id", "!=", record_id))

            try:
                svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                    "zero_sudo.user_websites_service_account"
                )
                env_group = self.env["user.websites.group"].with_user(svc_uid)
                env_user = self.env["res.users"].with_user(svc_uid)
            except AccessError:
                if self.env.su:
                    env_group = self.env["user.websites.group"]
                    env_user = self.env["res.users"]
                else:
                    raise

            group_collision = env_group.search_count(group_domain)
            user_collision = env_user.search_count([("website_slug", "=", slug)])

            if not user_collision and not group_collision:
                return slug

            slug = f"{base_slug}-{counter}"
            counter += 1

    @api.model_create_multi
    def create(self, vals_list):
        # Tested by [@ANCHOR: user_websites:test_group_site_creation]
        """
        Overrides create to automate the creation of the Odoo security group
        and intelligently generate or format the group's website slug.
        """
        groups_to_create_vals = []
        indices_needing_groups = []

        privilege = self.env.ref(
            "user_websites.privilege_user_websites", raise_if_not_found=False
        )
        category = self.env.ref(
            "user_websites.module_category_user_websites", raise_if_not_found=False
        )

        for i, vals in enumerate(vals_list):
            # Default Slug Generation
            if vals.get("website_slug"):
                vals["website_slug"] = slugify(vals["website_slug"])
            elif vals.get("name"):
                vals["website_slug"] = self._generate_unique_slug(vals["name"])

            # Auto-Create Security Group
            if "odoo_group_id" not in vals:
                group_name = vals.get("name", "New Group")
                group_vals = {
                    "name": f"Website Group: {group_name}",
                }

                if privilege:
                    group_vals["privilege_id"] = privilege.id
                elif category:
                    group_vals["category_id"] = category.id

                groups_to_create_vals.append(group_vals)
                indices_needing_groups.append(i)

        if groups_to_create_vals:
            svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                "zero_sudo.user_websites_service_account"
            )
            new_odoo_groups = (
                self.env["res.groups"].with_user(svc_uid).create(groups_to_create_vals)
            )
            for i, new_group in zip(indices_needing_groups, new_odoo_groups):
                vals_list[i]["odoo_group_id"] = new_group.id

        return super(UserWebsitesGroup, self).create(vals_list)

    def write(self, vals):
        old_slugs = {}
        # [@ANCHOR: group_slug_cache_invalidation]
        # Verified by [@ANCHOR: test_group_slug_cache_invalidation]
        if "website_slug" in vals:
            slugs = [group.website_slug for group in self if group.website_slug]
            if slugs:
                self.env["zero_sudo.security.utils"]._notify_cache_invalidation(
                    "user.websites.group", slugs
                )
                invalidate_model_cache(self.env, self._name)
                payload = json.dumps({"model": self._name})
                self.env.cr.execute(
                    "SELECT pg_notify(%s, %s)",
                    ("distributed_cache_invalidation", payload),
                )

            if vals.get("website_slug"):
                if len(self) == 1:
                    vals["website_slug"] = self._generate_unique_slug(
                        vals["website_slug"], record_id=self.id
                    )
                else:
                    vals["website_slug"] = slugify(vals["website_slug"])

            old_slugs = {
                group.id: group.website_slug for group in self if group.website_slug
            }

        try:
            result = super(UserWebsitesGroup, self).write(vals)
        except IntegrityError:
            self.env.cr.rollback()
            raise ValidationError(_("The Group Website Slug must be unique and valid."))

        # --- 301 Redirect Automation ---
        if "website_slug" in vals:
            # Send targeted NOTIFY to prevent global cache wipe
            slugs2 = [group.website_slug for group in self if group.website_slug]
            if slugs2:
                self.env["zero_sudo.security.utils"]._notify_cache_invalidation(
                    "user.websites.group", slugs2
                )

            svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                "zero_sudo.user_websites_service_account"
            )
            redirect_env = self.env["website.rewrite"].with_user(svc_uid)

            group_ids = self.ids
            blog_post_counts = {}
            if group_ids:
                blog_posts = (
                    self.env["blog.post"]
                    .with_user(svc_uid)
                    ._read_group(
                        [("user_websites_group_id", "in", group_ids)],
                        ["user_websites_group_id"],
                        ["__count"],
                    )
                )
                for group_owner, count in blog_posts:
                    blog_post_counts[group_owner.id] = count

            for group in self:
                old_slug = old_slugs.get(group.id)
                new_slug = group.website_slug
                if old_slug and new_slug and old_slug != new_slug:
                    redirects = [
                        {
                            "name": f"Redirect {old_slug} to {new_slug}",
                            "url_from": f"/{old_slug}",
                            "url_to": f"/{new_slug}",
                            "redirect_type": "301",
                            "website_id": False,
                        }
                    ]
                    if blog_post_counts.get(group.id, 0) > 0:
                        redirects.append(
                            {
                                "name": f"Redirect {old_slug} blog to {new_slug} blog",
                                "url_from": f"/{old_slug}/blog",
                                "url_to": f"/{new_slug}/blog",
                                "redirect_type": "301",
                                "website_id": False,
                            }
                        )
                    redirect_env.create(redirects)

        return result

    def unlink(self):
        # [@ANCHOR: group_slug_cache_invalidation_unlink]
        # Verified by [@ANCHOR: test_group_slug_cache_invalidation]
        slugs = [group.website_slug for group in self if group.website_slug]
        if slugs:
            self.env["zero_sudo.security.utils"]._notify_cache_invalidation(
                "user.websites.group", slugs
            )
            invalidate_model_cache(self.env, self._name)
            payload = json.dumps({"model": self._name})
            self.env.cr.execute(
                "SELECT pg_notify(%s, %s)", ("distributed_cache_invalidation", payload)
            )
        return super(UserWebsitesGroup, self).unlink()
