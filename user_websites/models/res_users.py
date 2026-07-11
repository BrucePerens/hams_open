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

from odoo.addons.distributed_redis_cache.redis_cache import distributed_cache
from odoo import models, fields, api, _
from odoo.exceptions import ValidationError, AccessError
from psycopg2 import IntegrityError
from odoo.addons.edge_routing.utils import RESERVED_SLUGS
import psycopg2
from odoo.modules.registry import Registry


BACKGROUND_EXECUTOR = ThreadPoolExecutor(max_workers=4)
_logger = logging.getLogger(__name__)


def _async_unpublish_content(db_name, user_ids):
    """Unpublishes user content in the background to prevent transaction lock exhaustion."""
    registry = Registry(db_name)
    cr = registry.cursor()
    try:
        # ADR-0001: Execute operations under a dedicated service account instead of SUPERUSER_ID
        cr.execute("SELECT id FROM res_users WHERE login = 'sys_provisioner'")
        row = cr.fetchone()
        svc_id = row[0] if row else 2
        env = odoo.api.Environment(cr, svc_id, {})
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
                if not os.environ.get("ODOO_DISABLE_SLEEPS"):
                    time.sleep(0.1)  # audit-ignore-sleep: Rate limiting background thread

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
                if not os.environ.get("ODOO_DISABLE_SLEEPS"):
                    time.sleep(0.1)  # audit-ignore-sleep: Rate limiting background thread

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
                if not os.environ.get("ODOO_DISABLE_SLEEPS"):
                    time.sleep(0.1)  # audit-ignore-sleep: Rate limiting background thread
        except (odoo.exceptions.AccessError, odoo.exceptions.ValidationError) as e:
            env.cr.rollback()
            logging.getLogger(__name__).warning(
                "Background unpublish business logic failure: %s", e
            )
        except Exception as e:  # audit-ignore-catch-all
            env.cr.rollback()
            logging.getLogger(__name__).error(
                "Fatal error during background unpublish: %s", e
            )
    finally:
        cr.close()


class ResUsers(models.Model):
    """
    Inherits from `res.users` to add features for personal user websites.
    """

    _inherit = ["res.users", "edge.routing.mixin"]
    _website_slug_format = models.Constraint("CHECK(website_slug IS NULL OR website_slug = '' OR website_slug ~ '^[a-z0-9\\-]+$')", 'The Website Slug can only contain lowercase letters, numbers, and hyphens.')

    def _register_hook(self):
        super(ResUsers, self)._register_hook()
        # Early initialization of sys_provisioner to satisfy cross-module dependencies
        # Runs before any XML data files are processed, bypassing Uninstalled Module parse errors
        with self.env.cr.savepoint():
            existing = (
                self.env["res.users"]
                .with_context(active_test=False)
                .search([("login", "=", "sys_provisioner")], limit=1)
            )
            if not existing:
                company_id = self.env.ref("base.main_company").id
                user = self.env["res.users"].create(
                    {
                        "name": "System Provisioner",
                        "login": "sys_provisioner",
                        "company_id": company_id,
                        "company_ids": [(4, company_id)],
                        "notification_type": "email",
                        "is_service_account": True,
                        "active": True,
                    }
                )
            else:
                user = existing

            xml_exists = self.env["ir.model.data"].search(
                [
                    ("module", "=", "user_websites"),
                    ("name", "=", "user_websites_service_account"),
                ],
                limit=1,
            )
            if not xml_exists:
                self.env["ir.model.data"].create(
                    {
                        "module": "user_websites",
                        "name": "user_websites_service_account",
                        "model": "res.users",
                        "res_id": user.id,
                        "noupdate": True,
                    }
                )

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

    @api.constrains("website_slug")
    def _check_reserved_slugs(self):
        for record in self:
            if record.website_slug and record.website_slug.lower() in RESERVED_SLUGS:
                raise ValidationError(
                    _("The slug '%s' is reserved and cannot be used.") % record.website_slug
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

    _website_slug_unique = models.Constraint("UNIQUE(website_slug)", "The Website Slug must be unique!")

    def _is_admin(self):
        """Helper to check if the user has administration rights."""
        return super()._is_admin() or self.has_group(
            "user_websites.group_user_websites_administrator"
        ) or self.has_group("base.group_system")

    @api.model
    @distributed_cache()
    def _get_user_id_by_slug(self, slug, override_svc_uid=None):
        if not slug:
            return False
        # ADR-0001 / Zero-Sudo: Use direct SQL to resolve the slug to an ID.
        # This prevents AccessError loops in public routes and avoids
        # transaction isolation issues in HttpCase tests.
        # It is safe because it only returns the ID; the caller must still
        # use the ORM to browse and read the record, which enforces ACLs.

        # We must flush the ORM cache first, otherwise test records created
        # in setUp() (which do not auto-commit) will be invisible to this raw SQL query.
        self.env.flush_all()
        self.env.cr.execute(
            "SELECT id FROM res_users WHERE website_slug = %s LIMIT 1", (slug,)
        )
        row = self.env.cr.fetchone()
        return row[0] if row else False

    @api.model_create_multi
    def create(self, vals_list):
        for vals in vals_list:
            if "website_slug" in vals and not vals["website_slug"]:
                vals["website_slug"] = False

        return super(ResUsers, self).create(vals_list)

    def write(self, vals):
        old_slugs = {}
        if "website_slug" in vals:
            old_slugs = {
                user.id: user.website_slug for user in self if user.website_slug
            }

        # --- Content Lifecycle Policy ---
        if "active" in vals and not vals["active"]:
            users_to_archive = self.ids
            is_test = vars(self.env.registry).get("test_cr") is not None
            if not is_test:
                db_name = self.env.cr.dbname
                self.env.cr.postcommit.add(
                    lambda: BACKGROUND_EXECUTOR.submit(
                        _async_unpublish_content, db_name, users_to_archive
                    )
                )
            else:
                try:
                    with self.env.cr.savepoint():
                        env_svc = self.env["zero_sudo.security.utils"]._get_service_env(
                            "user_websites.user_websites_service_account"
                        )
                except (AccessError, psycopg2.Error) as e:
                    if "not found" in str(e).lower():
                        env_svc = self.env
                    else:
                        raise
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
            if "website_slug" in vals and not vals["website_slug"]:
                vals["website_slug"] = False
            with self.env.cr.savepoint():
                result = super(ResUsers, self).write(vals)
        except IntegrityError:
            raise ValidationError(_("The Website Slug must be unique and valid."))

        # --- 301 Redirect Automation ---
        if "website_slug" in vals:
            try:
                with self.env.cr.savepoint():
                    env_svc = self.env["zero_sudo.security.utils"]._get_service_env(
                        "user_websites.user_websites_service_account"
                    )
                    redirect_env = env_svc["website.rewrite"]
            except (AccessError, psycopg2.Error) as e:
                if "not found" in str(e).lower():
                    env_svc = self.env
                    redirect_env = env_svc["website.rewrite"]
                else:
                    raise

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

        is_test = vars(self.env.registry).get("test_cr") is not None

        if is_test:
            env_svc = self.env["zero_sudo.security.utils"]._get_service_env(
                "user_websites.user_websites_service_account"
            )
            pages_batch = env_svc["website.page"].search(
                [("owner_user_id", "=", user_id)], limit=10000
            )
            pages_data = [
                {"name": p.name, "url": p.url, "content": p.arch} for p in pages_batch
            ]

            blogs_batch = env_svc["blog.post"].search(
                [("owner_user_id", "=", user_id)], limit=10000
            )
            blogs_data = [
                {
                    "name": b.name,
                    "content": b.content,
                    "published_date": str(b.post_date),
                }
                for b in blogs_batch
            ]

            reports_batch = env_svc["content.violation.report"].search(
                [("reported_by_user_id", "=", user_id)], limit=10000
            )
            reports_data = [
                {
                    "target_url": r.target_url,
                    "description": r.description,
                    "status": r.state,
                    "submitted_date": str(r.create_date),
                }
                for r in reports_batch
            ]

            appeals_batch = env_svc["content.violation.appeal"].search(
                [("user_id", "=", user_id)], limit=10000
            )
            appeals_data = [
                {
                    "reason": a.reason,
                    "status": a.state,
                    "submitted_date": str(a.create_date),
                }
                for a in appeals_batch
            ]

            def generate_pages():
                for item in pages_data:
                    yield item

            def generate_blogs():
                for item in blogs_data:
                    yield item

            def generate_reports():
                for item in reports_data:
                    yield item

            def generate_appeals():
                for item in appeals_data:
                    yield item

        else:

            def generate_pages():
                offset = 0
                while True:
                    with Registry(db_name).cursor() as cr:
                        cr.execute(
                            "SELECT id FROM res_users WHERE login = 'sys_provisioner'"
                        )
                        row = cr.fetchone()
                        svc_id = row[0] if row else 2
                        env = odoo.api.Environment(cr, svc_id, {})
                        env_svc = env["zero_sudo.security.utils"]._get_service_env(
                            "user_websites.user_websites_service_account"
                        )
                        batch = env_svc["website.page"].search(
                            [("owner_user_id", "=", user_id)], limit=1000, offset=offset
                        )
                        items = [
                            {"name": p.name, "url": p.url, "content": p.arch}
                            for p in batch
                        ]
                    if not items:
                        break
                    for item in items:
                        yield item
                    if len(items) < 1000:
                        break
                    offset += 1000

            def generate_blogs():
                offset = 0
                while True:
                    with Registry(db_name).cursor() as cr:
                        cr.execute(
                            "SELECT id FROM res_users WHERE login = 'sys_provisioner'"
                        )
                        row = cr.fetchone()
                        svc_id = row[0] if row else 2
                        env = odoo.api.Environment(cr, svc_id, {})
                        env_svc = env["zero_sudo.security.utils"]._get_service_env(
                            "user_websites.user_websites_service_account"
                        )
                        batch = env_svc["blog.post"].search(
                            [("owner_user_id", "=", user_id)], limit=1000, offset=offset
                        )
                        items = [
                            {
                                "name": b.name,
                                "content": b.content,
                                "published_date": str(b.post_date),
                            }
                            for b in batch
                        ]
                    if not items:
                        break
                    for item in items:
                        yield item
                    if len(items) < 1000:
                        break
                    offset += 1000

            def generate_reports():
                offset = 0
                while True:
                    with Registry(db_name).cursor() as cr:
                        cr.execute(
                            "SELECT id FROM res_users WHERE login = 'sys_provisioner'"
                        )
                        row = cr.fetchone()
                        svc_id = row[0] if row else 2
                        env = odoo.api.Environment(cr, svc_id, {})
                        env_svc = env["zero_sudo.security.utils"]._get_service_env(
                            "user_websites.user_websites_service_account"
                        )
                        batch = env_svc["content.violation.report"].search(
                            [("reported_by_user_id", "=", user_id)],
                            limit=1000,
                            offset=offset,
                        )
                        items = [
                            {
                                "target_url": r.target_url,
                                "description": r.description,
                                "status": r.state,
                                "submitted_date": str(r.create_date),
                            }
                            for r in batch
                        ]
                    if not items:
                        break
                    for item in items:
                        yield item
                    if len(items) < 1000:
                        break
                    offset += 1000

            def generate_appeals():
                offset = 0
                while True:
                    with Registry(db_name).cursor() as cr:
                        cr.execute(
                            "SELECT id FROM res_users WHERE login = 'sys_provisioner'"
                        )
                        row = cr.fetchone()
                        svc_id = row[0] if row else 2
                        env = odoo.api.Environment(cr, svc_id, {})
                        env_svc = env["zero_sudo.security.utils"]._get_service_env(
                            "user_websites.user_websites_service_account"
                        )
                        batch = env_svc["content.violation.appeal"].search(
                            [("user_id", "=", user_id)], limit=1000, offset=offset
                        )
                        items = [
                            {
                                "reason": a.reason,
                                "status": a.state,
                                "submitted_date": str(a.create_date),
                            }
                            for a in batch
                        ]
                    if not items:
                        break
                    for item in items:
                        yield item
                    if len(items) < 1000:
                        break
                    offset += 1000

        mro = self.__class__.__mro__
        start_idx = mro.index(ResUsers) + 1
        has_parent_method = any(
            "_get_gdpr_streamed_keys" in cls.__dict__ for cls in mro[start_idx:]
        )
        if has_parent_method:
            res = super()._get_gdpr_streamed_keys()
        else:
            res = {}
        res.update(
            {
                "pages": generate_pages,
                "blog_posts": generate_blogs,
                "submitted_reports": generate_reports,
                "appeals": generate_appeals,
            }
        )
        return res

    def _get_gdpr_export_data(self):
        # [@ANCHOR: res_users_gdpr_export]
        # Verified by [@ANCHOR: test_gdpr_export_hook]
        """
        Packages all the user's data and content into a dictionary so they can download it.
        """
        self.ensure_one()
        
        if hasattr(super(), "_get_gdpr_export_data"):
            res = super()._get_gdpr_export_data()
        else:
            res = {}

        if "user" not in res:
            res["user"] = {}

        res["user"].update({
            "name": self.name,
            "email": self.email,
            "website_slug": self.website_slug if self.website_slug else False,
        })
        return res

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
            except Exception as e:  # audit-ignore-catch-all
                logging.getLogger(__name__).warning(
                    "GDPR erasure concurrent update pages: %s", e
                )
                if (
                    "concurrent update" in str(e).lower()
                    or "serialization" in str(e).lower()
                    or "deadlock" in str(e).lower()
                ):
                    time.sleep(0.5)  # audit-ignore-sleep: Retry backoff
                    continue
                raise

            is_test = vars(self.env.registry).get("test_cr") is not None
            if not is_test:
                self.env.cr.commit()
            if len(pages) < 5000:
                break
            if not os.environ.get("ODOO_DISABLE_SLEEPS"):
                time.sleep(0.1)  # audit-ignore-sleep: Rate limiting background thread

        while True:
            posts = self.env["blog.post"].search(
                [("owner_user_id", "=", self.id)], limit=5000
            )
            if not posts:
                break
            try:
                with self.env.cr.savepoint():
                    env_svc["blog.post"].browse(posts.ids).unlink()
            except Exception as e:  # audit-ignore-catch-all
                logging.getLogger(__name__).warning(
                    "GDPR erasure concurrent update posts: %s", e
                )
                if (
                    "concurrent update" in str(e).lower()
                    or "serialization" in str(e).lower()
                    or "deadlock" in str(e).lower()
                ):
                    time.sleep(0.5)  # audit-ignore-sleep: Retry backoff
                    continue
                raise

            is_test = vars(self.env.registry).get("test_cr") is not None
            if not is_test:
                self.env.cr.commit()
            if len(posts) < 5000:
                break
            if not os.environ.get("ODOO_DISABLE_SLEEPS"):
                time.sleep(0.1)  # audit-ignore-sleep: Rate limiting background thread

        while True:
            blogs = self.env["blog.blog"].search(
                [("owner_user_id", "=", self.id)], limit=5000
            )
            if not blogs:
                break
            try:
                with self.env.cr.savepoint():
                    env_svc["blog.blog"].browse(blogs.ids).unlink()
            except Exception as e:  # audit-ignore-catch-all
                logging.getLogger(__name__).warning(
                    "GDPR erasure concurrent update blogs: %s", e
                )
                if (
                    "concurrent update" in str(e).lower()
                    or "serialization" in str(e).lower()
                    or "deadlock" in str(e).lower()
                ):
                    time.sleep(0.5)  # audit-ignore-sleep: Retry backoff
                    continue
                raise

            is_test = vars(self.env.registry).get("test_cr") is not None
            if not is_test:
                self.env.cr.commit()
            if len(blogs) < 5000:
                break
            if not os.environ.get("ODOO_DISABLE_SLEEPS"):
                time.sleep(0.1)  # audit-ignore-sleep: Rate limiting background thread

        # ADR-0001: All service account mutations must include appropriate context
        self.with_env(env_svc).write({"privacy_show_in_directory": False})

        # Enforce strict contract, let missing methods fail loudly if expected
        super()._execute_gdpr_erasure()
