# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).
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


# [@ANCHOR: user_websites:COMM_async_unpublish_content]
def _async_unpublish_content(db_name, user_ids):
    """Unpublishes user content in the background to prevent transaction lock exhaustion."""
    registry = Registry(db_name)
    cr = registry.cursor()
    try:
        # ADR-0001: Execute operations under a dedicated service account instead of SUPERUSER_ID
        cr.execute("SELECT id FROM res_users WHERE login = 'sys_provisioner'")
        row = cr.fetchone()
        if not row:
            raise ValueError("sys_provisioner missing")
        svc_id = row[0]
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
                    time.sleep(0.1)  # audit-ignore-sleep

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
                    time.sleep(0.1)  # audit-ignore-sleep

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
                    time.sleep(0.1)  # audit-ignore-sleep
        except (odoo.exceptions.AccessError, odoo.exceptions.ValidationError) as e:
            env.cr.rollback()
            logging.getLogger(__name__).warning(
                "Background unpublish business logic failure: %s", e
            )
        except (KeyError, ValueError) as e:  # audit-ignore-catch-all
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

    # [@ANCHOR: user_websites:COMM_register_hook]
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
        # Verified by [@ANCHOR: test_user_websites_self_writeable_fields]
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
    # [@ANCHOR: user_websites:COMM_res_users_check_reserved_slugs]
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

    # [@ANCHOR: user_websites:COMM_is_admin]
    def _is_admin(self):
        """Helper to check if the user has administration rights."""
        return super()._is_admin() or self.has_group(
            "user_websites.group_user_websites_administrator"
        ) or self.has_group("base.group_system")

    @api.model
    @distributed_cache()
    # [@ANCHOR: user_websites:COMM_get_user_id_by_slug]
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
            "SELECT res_id FROM user_websites_content_routing_view WHERE website_slug = %s AND res_model = 'res.users' LIMIT 1", (slug,)
        )
        row = self.env.cr.fetchone()
        return row[0] if row else False

    # [@ANCHOR: user_websites:COMM_res_users_create]
    @api.model_create_multi
    def create(self, vals_list):
        for vals in vals_list:
            if "website_slug" in vals and not vals["website_slug"]:
                vals["website_slug"] = False

        return super(ResUsers, self).create(vals_list)

    # [@ANCHOR: user_websites:COMM_res_users_write]
    def write(self, vals):
        old_slugs = {}
        if "website_slug" in vals:
            old_slugs = {
                user.id: user.website_slug for user in self if user.website_slug
            }

        try:
            if "website_slug" in vals and not vals["website_slug"]:
                vals["website_slug"] = False
            with self.env.cr.savepoint():
                result = super(ResUsers, self).write(vals)
        except IntegrityError as e:
            # Same bug class already fixed in user_websites_groups.py's write():
            # this used to catch every IntegrityError from the whole write(),
            # not just website_slug ones, and always re-raised the same
            # "slug must be unique" message -- mislabeling unrelated
            # constraint failures (e.g. a bad FK) as a slug problem. Only
            # relabel when the violated constraint is actually one of the
            # website_slug constraints declared on `edge.routing.mixin`.
            constraint_name = getattr(getattr(e, "diag", None), "constraint_name", None) or ""
            if "website_slug" in constraint_name:
                raise ValidationError(_("The Website Slug must be unique and valid."))
            raise

        # --- Content Lifecycle Policy ---
        # Adversarial security review, 2026-09-09: this block used to run
        # BEFORE super().write(vals) above, i.e. before the access-controlled
        # write that actually applies (or rejects) the deactivation. Scheduling
        # the postcommit unpublish job -- or, in test mode, unpublishing
        # synchronously with service-account-elevated privileges -- based
        # merely on the REQUESTED vals let a caller trigger real content
        # takedown for a user whose "active": False write never actually
        # succeeded: `Cursor.postcommit` is transaction-scoped, not
        # call-scoped, so any code elsewhere in the same request that caught
        # the resulting AccessError and let the overall transaction commit
        # anyway (a common batch/cron "skip records I can't touch" pattern)
        # would still fire the queued unpublish for content whose owner was
        # never actually deactivated. The test-mode synchronous branch was
        # worse: it ran immediately, with no savepoint of its own, so even a
        # locally-caught AccessError from the write below left the just-run
        # unpublish in place. Moving this block after a *successful*
        # super().write() ties the side effect to the authorized outcome, not
        # the request -- matching the "301 Redirect Automation" block below,
        # which already only runs once the write has taken effect.
        if "active" in vals and not vals["active"]:
            users_to_archive = self.ids
            is_test = self.env["zero_sudo.security.utils"]._is_test_mode()
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

    # [@ANCHOR: user_websites:COMM_get_page_limit]
    def _get_page_limit(self):
        self.ensure_one()
        limit = self.website_page_limit
        if not limit or limit <= 0:
            limit = self.env["zero_sudo.security.utils"]._get_system_param(
                "user_websites.global_website_page_limit", 100
            )
        return int(limit)

    # Adversarial security review, 2026-09-03: website.page enforces
    # _get_page_limit() on create() (website_page.py, [@ANCHOR:
    # website_page_quota_check]); blog.blog/blog.post had no equivalent at
    # all -- any authenticated user could create unbounded numbers of
    # either via direct RPC, each blog.post create also enqueuing a real
    # Cloudflare cache-purge and a distributed cache-invalidation notify, a
    # straightforward reachable-by-any-user resource-exhaustion gap.
    # Separate, smaller default limits since a real user typically needs
    # very few blogs but may reasonably write many posts within them.
    # [@ANCHOR: user_websites:COMM_get_blog_limit]
    def _get_blog_limit(self):
        self.ensure_one()
        return int(
            self.env["zero_sudo.security.utils"]._get_system_param(
                "user_websites.global_blog_limit", 5
            )
        )

    # [@ANCHOR: user_websites:COMM_get_blog_post_limit]
    def _get_blog_post_limit(self):
        self.ensure_one()
        return int(
            self.env["zero_sudo.security.utils"]._get_system_param(
                "user_websites.global_blog_post_limit", 500
            )
        )

    # [@ANCHOR: user_websites:COMM_get_gdpr_streamed_keys]
    def _get_gdpr_streamed_keys(self):
        """
        Returns a dictionary mapping JSON keys to generator functions.
        Used for streaming massive datasets (like QSOs) directly to the HTTP
        response to prevent OOM crashes during JSON serialization.
        """
        self.ensure_one()
        user_id = self.id
        db_name = self.env.cr.dbname

        is_test = self.env["zero_sudo.security.utils"]._is_test_mode()

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
                last_id = 0
                while True:
                    with Registry(db_name).cursor() as cr:
                        cr.execute(
                            "SELECT id FROM res_users WHERE login = 'sys_provisioner'"
                        )
                        row = cr.fetchone()
                        if not row:
                            raise ValueError("sys_provisioner missing")
                        svc_id = row[0]
                        env = odoo.api.Environment(cr, svc_id, {})
                        env_svc = env["zero_sudo.security.utils"]._get_service_env(
                            "user_websites.user_websites_service_account"
                        )
                        batch = env_svc["website.page"].search(
                            [("owner_user_id", "=", user_id), ("id", ">", last_id)], limit=1000, order="id asc"
                        )
                        items = [
                            {"name": p.name, "url": p.url, "content": p.arch}
                            for p in batch
                        ]
                        if batch:
                            last_id = batch[-1].id
                    if not items:
                        break
                    for item in items:
                        yield item
                    if len(items) < 1000:
                        break

            def generate_blogs():
                last_id = 0
                while True:
                    with Registry(db_name).cursor() as cr:
                        cr.execute(
                            "SELECT id FROM res_users WHERE login = 'sys_provisioner'"
                        )
                        row = cr.fetchone()
                        if not row:
                            raise ValueError("sys_provisioner missing")
                        svc_id = row[0]
                        env = odoo.api.Environment(cr, svc_id, {})
                        env_svc = env["zero_sudo.security.utils"]._get_service_env(
                            "user_websites.user_websites_service_account"
                        )
                        batch = env_svc["blog.post"].search(
                            [("owner_user_id", "=", user_id), ("id", ">", last_id)], limit=1000, order="id asc"
                        )
                        items = [
                            {
                                "name": b.name,
                                "content": b.content,
                                "published_date": str(b.post_date),
                            }
                            for b in batch
                        ]
                        if batch:
                            last_id = batch[-1].id
                    if not items:
                        break
                    for item in items:
                        yield item
                    if len(items) < 1000:
                        break

            def generate_reports():
                last_id = 0
                while True:
                    with Registry(db_name).cursor() as cr:
                        cr.execute(
                            "SELECT id FROM res_users WHERE login = 'sys_provisioner'"
                        )
                        row = cr.fetchone()
                        if not row:
                            raise ValueError("sys_provisioner missing")
                        svc_id = row[0]
                        env = odoo.api.Environment(cr, svc_id, {})
                        env_svc = env["zero_sudo.security.utils"]._get_service_env(
                            "user_websites.user_websites_service_account"
                        )
                        batch = env_svc["content.violation.report"].search(
                            [("reported_by_user_id", "=", user_id), ("id", ">", last_id)],
                            limit=1000,
                            order="id asc",
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
                        if batch:
                            last_id = batch[-1].id
                    if not items:
                        break
                    for item in items:
                        yield item
                    if len(items) < 1000:
                        break

            def generate_appeals():
                last_id = 0
                while True:
                    with Registry(db_name).cursor() as cr:
                        cr.execute(
                            "SELECT id FROM res_users WHERE login = 'sys_provisioner'"
                        )
                        row = cr.fetchone()
                        if not row:
                            raise ValueError("sys_provisioner missing")
                        svc_id = row[0]
                        env = odoo.api.Environment(cr, svc_id, {})
                        env_svc = env["zero_sudo.security.utils"]._get_service_env(
                            "user_websites.user_websites_service_account"
                        )
                        batch = env_svc["content.violation.appeal"].search(
                            [("user_id", "=", user_id), ("id", ">", last_id)], limit=1000, order="id asc"
                        )
                        items = [
                            {
                                "reason": a.reason,
                                "status": a.state,
                                "submitted_date": str(a.create_date),
                            }
                            for a in batch
                        ]
                        if batch:
                            last_id = batch[-1].id
                    if not items:
                        break
                    for item in items:
                        yield item
                    if len(items) < 1000:
                        break

        mro = self.__class__.__mro__
        _ = mro.index(ResUsers) + 1
        res = super()._get_gdpr_streamed_keys()
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

        # # Verified by [@ANCHOR: test_gdpr_export_hook]
        """
        Packages all the user's data and content into a dictionary so they can download it.
        """
        self.ensure_one()
        
        res = super()._get_gdpr_export_data()

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
            "zero_sudo.gdpr_service_internal"
        )

        # [@ANCHOR: gdpr_sudo_erasure]

        # # Verified by [@ANCHOR: test_gdpr_erasure_pages]

        # # Verified by [@ANCHOR: test_gdpr_erasure_posts]

        # Adversarial security review, 2026-09-09: the three searches below
        # used to run as `self.env` (the CALLER's own permissions) while the
        # matching unlink() already used `env_svc` (the elevated service
        # account) -- an inconsistent trust boundary compared to this same
        # file's own established Service Account Pattern (`_async_unpublish_
        # content` above searches AND writes via env_svc). Per compliance's
        # own test contract (test_execute_gdpr_erasure_uses_the_service_
        # account_not_the_caller, compliance/tests/test_gdpr_base.py), this
        # method must "succeed even when called by a low-privilege user" --
        # but website_page_group_rule/blog_post_group_rule only grant a
        # caller visibility into pages/posts owned by groups THEY belong to.
        # A caller who is not a member of the target user's own website
        # group (e.g. an admin-triggered erasure, or any future caller other
        # than the exact self-service `request.env.user._execute_gdpr_
        # erasure()` path) would have the search silently return zero rows,
        # so the `if not pages/posts/blogs: break` fires immediately and the
        # target's real content is never found -- and therefore never
        # erased -- even though env_svc (used only for the unlink) already
        # has full rights to delete it once found. Searching via env_svc
        # closes that gap: enumeration and deletion now share the same,
        # already-scoped-to-owner_user_id, elevated identity.
        while True:
            pages = env_svc["website.page"].search(
                [("owner_user_id", "=", self.id)], limit=5000
            )
            if not pages:
                break
            try:
                with self.env.cr.savepoint():
                    env_svc["website.page"].browse(pages.ids).unlink()  # audit-ignore-gdpr-hand-rolled-unlink: batched/savepoint-protected for datasets that can run into the thousands, see check_gdpr_erasure_uses_service_utility.py
            except (KeyError, ValueError) as e:  # audit-ignore-catch-all
                logging.getLogger(__name__).warning(
                    "GDPR erasure concurrent update pages: %s", e
                )
                if (
                    "concurrent update" in str(e).lower()
                    or "serialization" in str(e).lower()
                    or "deadlock" in str(e).lower()
                ):
                    time.sleep(0.5)  # audit-ignore-sleep
                    continue
                raise

            is_test = self.env["zero_sudo.security.utils"]._is_test_mode()
            if not is_test:
                self.env.cr.commit()
            if len(pages) < 5000:
                break
            if not os.environ.get("ODOO_DISABLE_SLEEPS"):
                time.sleep(0.1)  # audit-ignore-sleep

        while True:
            posts = env_svc["blog.post"].search(
                [("owner_user_id", "=", self.id)], limit=5000
            )
            if not posts:
                break
            try:
                with self.env.cr.savepoint():
                    env_svc["blog.post"].browse(posts.ids).unlink()  # audit-ignore-gdpr-hand-rolled-unlink: batched/savepoint-protected for datasets that can run into the thousands, see check_gdpr_erasure_uses_service_utility.py
            except (KeyError, ValueError) as e:  # audit-ignore-catch-all
                logging.getLogger(__name__).warning(
                    "GDPR erasure concurrent update posts: %s", e
                )
                if (
                    "concurrent update" in str(e).lower()
                    or "serialization" in str(e).lower()
                    or "deadlock" in str(e).lower()
                ):
                    time.sleep(0.5)  # audit-ignore-sleep
                    continue
                raise

            is_test = self.env["zero_sudo.security.utils"]._is_test_mode()
            if not is_test:
                self.env.cr.commit()
            if len(posts) < 5000:
                break
            if not os.environ.get("ODOO_DISABLE_SLEEPS"):
                time.sleep(0.1)  # audit-ignore-sleep

        while True:
            blogs = env_svc["blog.blog"].search(
                [("owner_user_id", "=", self.id)], limit=5000
            )
            if not blogs:
                break
            try:
                with self.env.cr.savepoint():
                    env_svc["blog.blog"].browse(blogs.ids).unlink()  # audit-ignore-gdpr-hand-rolled-unlink: batched/savepoint-protected for datasets that can run into the thousands, see check_gdpr_erasure_uses_service_utility.py
            except (KeyError, ValueError) as e:  # audit-ignore-catch-all
                logging.getLogger(__name__).warning(
                    "GDPR erasure concurrent update blogs: %s", e
                )
                if (
                    "concurrent update" in str(e).lower()
                    or "serialization" in str(e).lower()
                    or "deadlock" in str(e).lower()
                ):
                    time.sleep(0.5)  # audit-ignore-sleep
                    continue
                raise

            is_test = self.env["zero_sudo.security.utils"]._is_test_mode()
            if not is_test:
                self.env.cr.commit()
            if len(blogs) < 5000:
                break
            if not os.environ.get("ODOO_DISABLE_SLEEPS"):
                time.sleep(0.1)  # audit-ignore-sleep

        # ADR-0001: All service account mutations must include appropriate context
        self.with_env(env_svc).write({"privacy_show_in_directory": False})

        # Enforce strict contract, let missing methods fail loudly if expected
        super()._execute_gdpr_erasure()
