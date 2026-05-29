# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
"""
This file extends the built-in Odoo `res.users` model to add fields and logic
specific to the user websites functionality.
"""

import time
import os
import odoo
import logging
from concurrent.futures import ThreadPoolExecutor
from odoo import models, fields, api, _
from odoo.exceptions import ValidationError, AccessError
from psycopg2 import IntegrityError
from odoo.modules.registry import Registry
from ..utils import slugify, RESERVED_SLUGS

BACKGROUND_EXECUTOR = ThreadPoolExecutor(max_workers=4)


def _async_unpublish_content(db_name, user_ids):
    """Unpublishes user content in the background to prevent transaction lock exhaustion."""
    registry = Registry(db_name)
    cr = registry.cursor()
    try:
        # ADR-0001: Execute operations under a dedicated service account instead of SUPERUSER_ID
        env = odoo.api.Environment(cr, odoo.SUPERUSER_ID, {})
        try:
            env_svc = env["zero_sudo.security.utils"]._get_service_env(
                "user_websites.user_websites_service_account"
            )

            while True:
                pages = env_svc["website.page"].search(
                    [
                        ("owner_user_id", "in", user_ids),
                        ("website_published", "=", True),
                    ],
                    limit=5000,
                )
                if not pages:
                    break
                pages.write({"website_published": False})
                env.cr.commit()
                if len(pages) < 5000:
                    break
                if not os.environ.get('ODOO_DISABLE_SLEEPS'): time.sleep(0.1) # audit-ignore-sleep: Rate limiting background thread  # fmt: skip

            while True:
                posts = env_svc["blog.post"].search(
                    [
                        ("owner_user_id", "in", user_ids),
                        ("is_published", "=", True),
                    ],
                    limit=5000,
                )
                if not posts:
                    break
                posts.write({"is_published": False})
                env.cr.commit()
                if len(posts) < 5000:
                    break
                if not os.environ.get('ODOO_DISABLE_SLEEPS'): time.sleep(0.1) # audit-ignore-sleep: Rate limiting background thread  # fmt: skip

            while True:
                blogs = env_svc["blog.blog"].search(
                    [("owner_user_id", "in", user_ids)], limit=5000
                )
                if not blogs:
                    break
                blogs.write({"active": False})
                env.cr.commit()
                if len(blogs) < 5000:
                    break
                if not os.environ.get('ODOO_DISABLE_SLEEPS'): time.sleep(0.1) # audit-ignore-sleep: Rate limiting background thread  # fmt: skip
        except (odoo.exceptions.AccessError, odoo.exceptions.ValidationError) as e:
            env.cr.rollback()
            logging.getLogger(__name__).warning("Background unpublish business logic failure: %s", e)
        except Exception: # audit-ignore-catch-all
            env.cr.rollback()
            logging.getLogger(__name__).exception("Fatal error during background unpublish")
    finally:
        cr.close()


class ResUsers(models.Model):
    """
    Inherits from `res.users` to add features for personal user websites.
    """

    _inherit = "res.users"

    @property
    def SELF_WRITEABLE_FIELDS(self):
        """ADR-0015: Self-Writeable Fields Idiom"""
        return super().SELF_WRITEABLE_FIELDS + [
            "privacy_show_in_directory",
            "website_slug",
        ]

    # --- Field Definitions ---
    website_slug = fields.Char(
        string="Website Slug",
        index="trigram",
        help="The URL-friendly identifier for the user's site. Alphanumeric and hyphens only.",
    )

    website_page_limit = fields.Integer(
        string="Website Page Limit",
        help="Maximum number of pages this user can create. If 0, the global limit is used.",
    )

    privacy_show_in_directory = fields.Boolean(
        string="Show in Public Directory",
        help="If checked, a link to this user's website will appear in the public community directory.",
        default=False,
    )

    # --- Inverse Relationships (Bidirectional Integrity) ---
    user_websites_page_ids = fields.One2many(
        "website.page",
        "owner_user_id",
        string="Owned Website Pages",
        help="Pages owned by this user.",
    )

    user_websites_blog_post_ids = fields.One2many(
        "blog.post",
        "owner_user_id",
        string="Owned Blog Posts",
        help="Blog posts authored by this user.",
    )

    submitted_violation_report_ids = fields.One2many(
        "content.violation.report",
        "reported_by_user_id",
        string="Submitted Violation Reports",
        help="Reports submitted by this user.",
    )

    received_violation_report_ids = fields.One2many(
        "content.violation.report",
        "content_owner_id",
        string="Received Violation Reports",
        help="Reports filed against content owned by this user.",
    )

    appeal_ids = fields.One2many(
        "content.violation.appeal", "user_id", string="Moderation Appeals"
    )

    # --- Odoo 19 Constraint Syntax ---
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
                    % record.website_slug
                )

    # --- Slug Generation & Management ---

    @api.model
    def _generate_unique_slug(self, base_string, record_id=False):
        """
        Generates a URL-safe, globally unique slug. Cross-references reserved routes,
        other users, and groups.
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

            user_domain = [("website_slug", "=", slug)]
            if record_id:
                user_domain.append(("id", "!=", record_id))

            try:
                env_svc = self.env["zero_sudo.security.utils"]._get_service_env(
                    "user_websites.user_websites_service_account"
                )
                env_user = env_svc["res.users"]
                env_group = env_svc["user.websites.group"]
            except AccessError:
                if self.env.su:
                    env_user = self.env["res.users"]
                    env_group = self.env["user.websites.group"]
                else:
                    raise

            user_collision = env_user.search_count(user_domain)
            group_collision = env_group.search_count([("website_slug", "=", slug)])

            if not user_collision and not group_collision:
                # TOCTOU FIX: If it looks clear, lock the transaction to prevent a concurrent writer
                # from snagging it before we finish returning and inserting.
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

    @api.model_create_multi
    def create(self, vals_list):
        """
        Intercept creation to inject a default generated slug if none was explicitly provided.
        """
        for vals in vals_list:
            if vals.get("website_slug"):
                vals["website_slug"] = slugify(vals["website_slug"])
            elif vals.get("name"):
                vals["website_slug"] = self._generate_unique_slug(vals["name"])

        return super(ResUsers, self).create(vals_list)

    def write(self, vals):
        old_slugs = {}
        if "website_slug" in vals:
            # Safely format the incoming slug directly
            if vals.get("website_slug"):
                if len(self) == 1:
                    vals["website_slug"] = self._generate_unique_slug(
                        vals["website_slug"], record_id=self.id
                    )
                else:
                    # If bulk updating, enforce formatting but let DB handle collision detection
                    vals["website_slug"] = slugify(vals["website_slug"])

            old_slugs = {
                user.id: user.website_slug for user in self if user.website_slug
            }

        # --- Content Lifecycle Policy ---
        if "active" in vals and not vals["active"]:
            users_to_archive = self.ids
            if not odoo.tools.config.get("test_enable"):
                db_name = self.env.cr.dbname
                BACKGROUND_EXECUTOR.submit(
                    _async_unpublish_content, db_name, users_to_archive
                )
            else:
                env_svc = self.env["zero_sudo.security.utils"]._get_service_env(
                    "user_websites.user_websites_service_account"
                )
                while True:
                    pages = env_svc["website.page"].search(
                        [
                            ("owner_user_id", "in", users_to_archive),
                            ("website_published", "=", True),
                        ],
                        limit=5000,
                    )
                    if not pages:
                        break
                    pages.write({"website_published": False})
                while True:
                    posts = env_svc["blog.post"].search(
                        [
                            ("owner_user_id", "in", users_to_archive),
                            ("is_published", "=", True),
                        ],
                        limit=5000,
                    )
                    if not posts:
                        break
                    posts.write({"is_published": False})

                while True:
                    blogs = env_svc["blog.blog"].search(
                        [("owner_user_id", "in", users_to_archive)], limit=5000
                    )
                    if not blogs:
                        break
                    blogs.write({"active": False})
                    if len(blogs) < 5000:
                        break

        try:
            with self.env.cr.savepoint():
                result = super(ResUsers, self).write(vals)
        except IntegrityError:
            raise ValidationError(_("The Website Slug must be unique and valid."))

        # --- 301 Redirect Automation ---
        if "website_slug" in vals:
            env_svc = self.env["zero_sudo.security.utils"]._get_service_env(
                "user_websites.user_websites_service_account"
            )
            redirect_env = env_svc["website.rewrite"]

            user_ids = self.ids
            blog_post_counts = {}
            if user_ids:
                blog_posts = env_svc["blog.post"]._read_group(
                    [("owner_user_id", "in", user_ids)],
                    ["owner_user_id"],
                    ["__count"],
                )
                for owner, count in blog_posts:
                    blog_post_counts[owner.id] = count

            for user in self:
                old_slug = old_slugs.get(user.id)
                new_slug = user.website_slug
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
                    if blog_post_counts.get(user.id, 0) > 0:
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

    # --- Business & GDPR Extensible Methods ---

    def _get_page_limit(self):
        self.ensure_one()
        limit = self.website_page_limit
        if not limit or limit <= 0:
            limit = self.env["zero_sudo.security.utils"]._get_system_param(
                "user_websites.global_website_page_limit", 100
            )
        return int(limit)

    def _get_gdpr_streamed_keys(self):
        """
        Returns a dictionary mapping JSON keys to generator functions.
        Used for streaming massive datasets (like QSOs) directly to the HTTP
        response to prevent OOM crashes during JSON serialization.
        """
        self.ensure_one()
        user_id = self.id
        db_name = self.env.cr.dbname
        is_test = odoo.tools.config.get("test_enable")

        if is_test:
            env_svc = self.env["zero_sudo.security.utils"]._get_service_env(
                "user_websites.user_websites_service_account"
            )
            pages_batch = env_svc["website.page"].search([("owner_user_id", "=", user_id)], limit=10000)
            pages_data = [{"name": p.name, "url": p.url, "content": p.arch} for p in pages_batch]

            blogs_batch = env_svc["blog.post"].search([("owner_user_id", "=", user_id)], limit=10000)
            blogs_data = [{"name": b.name, "content": b.content, "published_date": str(b.post_date)} for b in blogs_batch]

            reports_batch = env_svc["content.violation.report"].search([("reported_by_user_id", "=", user_id)], limit=10000)
            reports_data = [{"target_url": r.target_url, "description": r.description, "status": r.state, "submitted_date": str(r.create_date)} for r in reports_batch]

            appeals_batch = env_svc["content.violation.appeal"].search([("user_id", "=", user_id)], limit=10000)
            appeals_data = [{"reason": a.reason, "status": a.state, "submitted_date": str(a.create_date)} for a in appeals_batch]

            def generate_pages():
                for item in pages_data: yield item
            def generate_blogs():
                for item in blogs_data: yield item
            def generate_reports():
                for item in reports_data: yield item
            def generate_appeals():
                for item in appeals_data: yield item
        else:
            def generate_pages():
                offset = 0
                while True:
                    with Registry(db_name).cursor() as cr:
                        env = odoo.api.Environment(cr, odoo.SUPERUSER_ID, {})
                        env_svc = env["zero_sudo.security.utils"]._get_service_env("user_websites.user_websites_service_account")
                        batch = env_svc["website.page"].search([("owner_user_id", "=", user_id)], limit=1000, offset=offset)
                        items = [{"name": p.name, "url": p.url, "content": p.arch} for p in batch]
                    if not items: break
                    for item in items: yield item
                    if len(items) < 1000: break
                    offset += 1000

            def generate_blogs():
                offset = 0
                while True:
                    with Registry(db_name).cursor() as cr:
                        env = odoo.api.Environment(cr, odoo.SUPERUSER_ID, {})
                        env_svc = env["zero_sudo.security.utils"]._get_service_env("user_websites.user_websites_service_account")
                        batch = env_svc["blog.post"].search([("owner_user_id", "=", user_id)], limit=1000, offset=offset)
                        items = [{"name": b.name, "content": b.content, "published_date": str(b.post_date)} for b in batch]
                    if not items: break
                    for item in items: yield item
                    if len(items) < 1000: break
                    offset += 1000

            def generate_reports():
                offset = 0
                while True:
                    with Registry(db_name).cursor() as cr:
                        env = odoo.api.Environment(cr, odoo.SUPERUSER_ID, {})
                        env_svc = env["zero_sudo.security.utils"]._get_service_env("user_websites.user_websites_service_account")
                        batch = env_svc["content.violation.report"].search([("reported_by_user_id", "=", user_id)], limit=1000, offset=offset)
                        items = [{"target_url": r.target_url, "description": r.description, "status": r.state, "submitted_date": str(r.create_date)} for r in batch]
                    if not items: break
                    for item in items: yield item
                    if len(items) < 1000: break
                    offset += 1000

            def generate_appeals():
                offset = 0
                while True:
                    with Registry(db_name).cursor() as cr:
                        env = odoo.api.Environment(cr, odoo.SUPERUSER_ID, {})
                        env_svc = env["zero_sudo.security.utils"]._get_service_env("user_websites.user_websites_service_account")
                        batch = env_svc["content.violation.appeal"].search([("user_id", "=", user_id)], limit=1000, offset=offset)
                        items = [{"reason": a.reason, "status": a.state, "submitted_date": str(a.create_date)} for a in batch]
                    if not items: break
                    for item in items: yield item
                    if len(items) < 1000: break
                    offset += 1000

        res = getattr(super(), "_get_gdpr_streamed_keys", lambda: {})()
        res.update({
            "pages": generate_pages,
            "blog_posts": generate_blogs,
            "submitted_reports": generate_reports,
            "appeals": generate_appeals,
        })
        return res

    def _get_gdpr_export_data(self):
        # [@ANCHOR: res_users_gdpr_export]
        # Verified by [@ANCHOR: test_gdpr_export_hook]
        """
        Packages all the user's data and content into a dictionary so they can download it.
        """
        self.ensure_one()

        return {
            "user": {
                "name": self.name,
                "email": self.email,
                "website_slug": self.website_slug,
            }
        }

    def _execute_gdpr_erasure(self):
        """
        Permanently deletes all content created by the user to comply with GDPR.
        """
        self.ensure_one()
        env_svc = self.env["zero_sudo.security.utils"]._get_service_env(
            "user_websites.user_websites_service_account"
        )

        # [@ANCHOR: gdpr_sudo_erasure]
        # Verified by [@ANCHOR: test_gdpr_erasure_pages]
        # Verified by [@ANCHOR: test_gdpr_erasure_posts]
        while True:
            pages = self.env["website.page"].search(
                [("owner_user_id", "=", self.id)], limit=5000
            )
            if not pages:
                break
            try:
                with self.env.cr.savepoint():
                    env_svc["website.page"].browse(pages.ids).unlink()
            except Exception as e: # audit-ignore-catch-all
                logging.getLogger(__name__).warning("GDPR erasure concurrent update pages: %s", e)
                if 'concurrent update' in str(e).lower() or 'serialization' in str(e).lower() or 'deadlock' in str(e).lower():
                    time.sleep(0.5) # audit-ignore-sleep: Retry backoff
                    continue
                raise
            if not odoo.tools.config.get("test_enable"):
                self.env.cr.commit()
            if len(pages) < 5000:
                break
            if not os.environ.get("ODOO_DISABLE_SLEEPS"):
                time.sleep(0.1) # audit-ignore-sleep: Rate limiting background thread  # fmt: skip

        while True:
            posts = self.env["blog.post"].search(
                [("owner_user_id", "=", self.id)], limit=5000
            )
            if not posts:
                break
            try:
                with self.env.cr.savepoint():
                    env_svc["blog.post"].browse(posts.ids).unlink()
            except Exception as e: # audit-ignore-catch-all
                logging.getLogger(__name__).warning("GDPR erasure concurrent update posts: %s", e)
                if 'concurrent update' in str(e).lower() or 'serialization' in str(e).lower() or 'deadlock' in str(e).lower():
                    time.sleep(0.5) # audit-ignore-sleep: Retry backoff
                    continue
                raise
            if not odoo.tools.config.get("test_enable"):
                self.env.cr.commit()
            if len(posts) < 5000:
                break
            if not os.environ.get("ODOO_DISABLE_SLEEPS"):
                time.sleep(0.1) # audit-ignore-sleep: Rate limiting background thread  # fmt: skip

        while True:
            blogs = self.env["blog.blog"].search(
                [("owner_user_id", "=", self.id)], limit=5000
            )
            if not blogs:
                break
            try:
                with self.env.cr.savepoint():
                    env_svc["blog.blog"].browse(blogs.ids).unlink()
            except Exception as e: # audit-ignore-catch-all
                logging.getLogger(__name__).warning("GDPR erasure concurrent update blogs: %s", e)
                if 'concurrent update' in str(e).lower() or 'serialization' in str(e).lower() or 'deadlock' in str(e).lower():
                    time.sleep(0.5) # audit-ignore-sleep: Retry backoff
                    continue
                raise
            if not odoo.tools.config.get("test_enable"):
                self.env.cr.commit()
            if len(blogs) < 5000:
                break
            if not os.environ.get("ODOO_DISABLE_SLEEPS"):
                time.sleep(0.1) # audit-ignore-sleep: Rate limiting background thread  # fmt: skip

        # ADR-0001: All service account mutations must include appropriate context
        self.with_env(env_svc).write({"privacy_show_in_directory": False})

        if hasattr(super(), "_execute_gdpr_erasure"):
            super()._execute_gdpr_erasure()
