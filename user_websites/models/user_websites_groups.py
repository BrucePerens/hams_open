# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""
This file defines the Odoo model for User Websites Groups.
"""

from odoo import models, fields, api, _
from odoo.exceptions import ValidationError
from odoo.addons.distributed_redis_cache.redis_cache import notify_model_invalidation
from odoo.addons.edge_routing.utils import RESERVED_SLUGS
from psycopg2 import IntegrityError
import psycopg2
import logging
from .res_users import BACKGROUND_EXECUTOR
import time
import os
import odoo
from odoo.modules.registry import Registry

_logger = logging.getLogger(__name__)


# [@ANCHOR: user_websites:COMM_async_unpublish_group_content]
def _async_unpublish_group_content(db_name, group_ids):
    """Unpublishes group content in the background to prevent transaction lock exhaustion."""
    # Registry(db_name) and registry.cursor() used to sit outside the try
    # block below -- this function runs in a background thread submitted
    # via BACKGROUND_EXECUTOR.submit() whose Future is never awaited, so
    # a failure acquiring the registry/cursor itself crashed the thread
    # with ZERO log output at all, indistinguishable from the function
    # never having been scheduled. Found while investigating
    # test_group_moderation.py::test_01_group_suspension's timeout (the
    # real bug there turned out to be in the test's own poll loop, not
    # here -- see that file's fix -- but this gap was real regardless:
    # any actual registry/cursor acquisition failure would still vanish
    # silently without it).
    try:
        registry = Registry(db_name)
        cr = registry.cursor()
    except Exception:  # audit-ignore-catch-all: Tested by [@ANCHOR: user_websites_async_unpublish_registry_failure]  # fmt: skip
        _logger.exception("Async unpublish group content failed to acquire a registry/cursor")
        return
    try:
        cr.execute(
            "SELECT res_id FROM ir_model_data WHERE module = %s AND name = %s",
            ("user_websites", "user_websites_service_account"),
        )
        row = cr.fetchone()
        if not row:
            return
        svc_id = row[0]
        env = odoo.api.Environment(cr, svc_id, {})
        try:
            env_svc = env["zero_sudo.security.utils"]._get_service_env(
                "user_websites.user_websites_service_account"
            )

            groups = env_svc["user.websites.group"].search(
                [("id", "in", group_ids)], limit=10000
            )
            company_groups = {}
            for g in groups:
                if g.company_id:
                    company_groups.setdefault(g.company_id.id, []).append(g.id)

            def _unpublish_for_company(company_id, comp_group_ids):
                # env_svc is an Environment, not a recordset -- it has no
                # with_company() of its own (that's a recordset method).
                # Switch the active company the same way with_company()
                # does under the hood: put company_id first in
                # allowed_company_ids.
                allowed_company_ids = list(env_svc.context.get("allowed_company_ids") or [])
                if company_id in allowed_company_ids:
                    allowed_company_ids.remove(company_id)
                allowed_company_ids.insert(0, company_id)
                company_env = env_svc(
                    context=dict(env_svc.context, allowed_company_ids=allowed_company_ids)
                )
                while True:
                    pages = company_env["website.page"].search(
                        [
                            ("user_websites_group_id", "in", comp_group_ids),
                            "|",
                            ("is_published", "=", True),
                            ("website_published", "=", True),
                        ],
                        limit=5000,
                    )
                    if not pages:
                        break
                    pages.with_context(mail_notrack=True).write(
                        {"is_published": False, "website_published": False}
                    )
                    env.cr.commit()
                    if len(pages) < 5000:
                        break
                    if not os.environ.get("ODOO_DISABLE_SLEEPS"):
                        time.sleep(
                            0.1
                        )  # audit-ignore-sleep: Rate limiting background thread

                while True:
                    posts = company_env["blog.post"].search(
                        [
                            ("user_websites_group_id", "in", comp_group_ids),
                            ("is_published", "=", True),
                        ],
                        limit=5000,
                    )
                    if not posts:
                        break
                    posts.with_context(mail_notrack=True).write({"is_published": False})
                    env.cr.commit()
                    if len(posts) < 5000:
                        break
                    if not os.environ.get("ODOO_DISABLE_SLEEPS"):
                        time.sleep(0.1)  # audit-ignore-sleep

            for company_id, comp_group_ids in company_groups.items():
                try:
                    _unpublish_for_company(company_id, comp_group_ids)
                except psycopg2.Error:
                    # A DB-level error poisons the rest of this transaction
                    # (Postgres refuses further statements on an aborted
                    # transaction) -- re-raise so the outer psycopg2.Error
                    # handler logs it and the whole function bails out,
                    # exactly as before this per-company isolation was added.
                    raise
                except Exception:
                    # Found in bug-hunt review: before this try/except, one
                    # company raising here (e.g. a future ir.rule change
                    # scoping website.page/blog.post by company would make
                    # `allowed_company_ids=[company_id]` raise AccessError
                    # for any company outside this service account's own
                    # real scope -- today just `base.main_company`, per
                    # data/user_websites_data.xml -- see
                    # hams_shared/docs/odoo_orm_reference.md's non-sudo
                    # allowed_company_ids note) would hit the bare `except
                    # Exception` far below and abort every *other* company's
                    # unpublish in the same batch too, not just the failing
                    # one. Isolate per company: log and continue, so one
                    # company's failure can't block unrelated companies'
                    # suspensions from taking effect.
                    _logger.exception(
                        "Async unpublish group content failed for company id "
                        "%s (group_ids=%s) -- continuing with any other "
                        "companies in this batch",
                        company_id,
                        comp_group_ids,
                    )

        finally:
            env.cr.rollback()
    except psycopg2.Error as e:  # audit-ignore-catch-all
        _logger.error("Async unpublish group content failed: %s", e)
    except Exception:  # audit-ignore-catch-all: Tested by [@ANCHOR: user_websites_async_unpublish_catch_all]  # fmt: skip
        # This runs in a background thread submitted via
        # BACKGROUND_EXECUTOR.submit() -- its Future is never awaited, so
        # any exception here would otherwise vanish silently instead of
        # ever reaching a caller. A non-psycopg2.Error bug here (e.g. a
        # bad env_svc.with_company() call once tried, since env_svc is an
        # Environment, not a recordset) previously meant this whole
        # feature failed 100% of the time with zero visibility.
        _logger.exception("Async unpublish group content crashed unexpectedly")
    finally:
        cr.close()


class UserWebsitesGroup(models.Model):
    """
    Represents a group of users who can manage a shared website.
    """

    _name = "user.websites.group"
    _description = "User Websites Group"
    _inherit = ["mail.thread", "mail.activity.mixin", "edge.routing.mixin"]

    # --- Fields Definition ---
    name = fields.Char(string="Group Name", required=True, tracking=True)

    violation_strike_count = fields.Integer(
        string="Violation Strikes",
        default=0,
        help="Number of upheld content violations. Hitting 3 triggers an automatic suspension.",
    )
    is_suspended_from_websites = fields.Boolean(
        string="Suspended from Websites",
        default=False,
        readonly=True,
        tracking=True,
        help="If true, this group's websites are unpublished and they cannot manage content.",
    )

    appeal_ids = fields.One2many(
        "content.violation.appeal",
        "group_id",
        string="Moderation Appeals",
        help="Appeals submitted on behalf of this group.",
    )

    odoo_group_id = fields.Many2one(
        "res.groups",
        string="Linked Odoo Group",
        required=True,
        ondelete="cascade",
        help="The Odoo security group associated with this website.",
    )

    @api.constrains("website_slug")
    # [@ANCHOR: user_websites:COMM_group_check_reserved_slugs]
    def _check_reserved_slugs(self):
        for record in self:
            if record.website_slug and record.website_slug.lower() in RESERVED_SLUGS:
                raise ValidationError(
                    _("The slug '%s' is reserved and cannot be used.")
                    % record.website_slug
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

    blog_blog_ids = fields.One2many(
        "blog.blog",
        "user_websites_group_id",
        string="Group Blogs",
    )

    company_id = fields.Many2one(
        "res.company",
        string="Company",
        required=True,
        default=lambda self: self.env.company,
    )

    # [@ANCHOR: user_websites:COMM_group_create]
    @api.model_create_multi
    def create(self, vals_list):
        # # Tested by [@ANCHOR: user_websites:test_group_site_creation]
        """
        Overrides create to automate the creation of the Odoo security group
        and intelligently generate or format the group's website slug.
        """
        groups_to_create_vals = []
        indices_needing_groups = []

        self.env.cr.execute(
            "SELECT res_id FROM ir_model_data WHERE module=%s AND name=%s",
            ("user_websites", "privilege_user_websites"),
        )
        row = self.env.cr.fetchone()
        privilege_id = row[0] if row else False

        self.env.cr.execute(
            "SELECT res_id FROM ir_model_data WHERE module=%s AND name=%s",
            ("user_websites", "module_category_user_websites"),
        )
        row = self.env.cr.fetchone()
        category_id = row[0] if row else False

        for i, vals in enumerate(vals_list):
            # Auto-Create Security Group
            if "odoo_group_id" not in vals:
                group_name = vals.get("name", "New Group")
                group_vals = {
                    "name": f"Website Group: {group_name}",
                }

                if privilege_id:
                    group_vals["privilege_id"] = privilege_id
                elif category_id:
                    # BUG (found in bug-hunt review): this used to assign
                    # `category_id` here too -- but `category_id` is an
                    # `ir.module.category` record id (looked up above from
                    # 'module_category_user_websites'), while
                    # `res.groups.privilege_id` is a Many2one to the
                    # completely different `res.groups.privilege` model.
                    # Writing an `ir.module.category` id into that column
                    # would either violate the FK constraint (raising a
                    # confusing raw IntegrityError instead of ever reaching
                    # this method's own caller cleanly) or, worse, silently
                    # point at whatever unrelated `res.groups.privilege` row
                    # happens to share that same numeric id. Only fall back
                    # to leaving privilege_id unset (it isn't required) and
                    # log loudly -- this branch should be unreachable in a
                    # normally-installed module (both records load from the
                    # same XML file), so hitting it at all means the
                    # module's own data didn't load as expected.
                    _logger.warning(
                        "user_websites: expected privilege record "
                        "'user_websites.privilege_user_websites' not found "
                        "(only its module category was) while auto-creating "
                        "a security group -- leaving privilege_id unset "
                        "rather than assigning module_category_user_websites's "
                        "id, which belongs to a different model."
                    )

                groups_to_create_vals.append(group_vals)
                indices_needing_groups.append(i)

        if groups_to_create_vals:
            svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                "user_websites.user_websites_service_account"
            )
            new_odoo_groups = (
                self.env["res.groups"].with_user(svc_uid).create(groups_to_create_vals)
            )
            for i, new_group in zip(indices_needing_groups, new_odoo_groups):
                vals_list[i]["odoo_group_id"] = new_group.id

        return super(UserWebsitesGroup, self).create(vals_list)

    # [@ANCHOR: user_websites:COMM_group_write]
    def write(self, vals):
        old_slugs = {}
        if "website_slug" in vals:
            old_slugs = {
                group.id: group.website_slug for group in self if group.website_slug
            }

        try:
            with self.env.cr.savepoint():
                result = super(UserWebsitesGroup, self).write(vals)
        except IntegrityError as e:
            # BUG (found in bug-hunt review): this used to catch every
            # IntegrityError from the whole write() -- not just website_slug
            # ones -- and always re-raised the same "slug must be unique"
            # message. `vals` can carry any field (e.g. a bad `odoo_group_id`
            # FK, or some other constraint entirely), so a completely
            # unrelated integrity failure was being mislabeled as a slug
            # problem, hiding the real cause from whoever has to debug it.
            # Only relabel the error when the violated constraint is
            # actually one of the two website_slug constraints declared on
            # `edge.routing.mixin` (`_website_slug_unique`/
            # `_website_slug_format`, named `<table>_website_slug_*` in the
            # DB per odoo/orm/table_objects.py's TableObject.full_name) --
            # otherwise re-raise the original error so it fails loudly with
            # its own real cause intact.
            constraint_name = getattr(getattr(e, "diag", None), "constraint_name", None) or ""
            if "website_slug" in constraint_name:
                raise ValidationError(_("The Group Website Slug must be unique and valid."))
            raise

        # --- 301 Redirect Automation ---
        if "website_slug" in vals:
            svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                "user_websites.user_websites_service_account"
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

    # [@ANCHOR: user_websites:COMM_action_suspend_group_websites]
    def action_suspend_group_websites(self):
        """Forcefully unpublishes all group content and flags them as suspended."""
        group_ids = self.ids
        is_test = self.env["zero_sudo.security.utils"]._is_test_mode()

        if not is_test:
            db_name = self.env.cr.dbname
            # Bug-hunt fix, 2026-09-09: defer via postcommit, matching the
            # identical fix already applied to res_users_moderation.py's
            # action_suspend_user_websites for the same class of function.
            # Firing this immediately let the per-group suspension loop
            # below still raise (e.g. message_post() failing for one group
            # in a multi-group batch) and roll back this whole transaction
            # -- including every is_suspended_from_websites write and audit
            # message -- while the background unpublish, on its own
            # separate DB connection, would still commit. postcommit only
            # runs if this transaction's own commit() actually happens.
            self.env.cr.postcommit.add(
                lambda: BACKGROUND_EXECUTOR.submit(
                    _async_unpublish_group_content, db_name, group_ids
                )
            )
        else:
            svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                "user_websites.user_websites_service_account"
            )
            while True:
                pages = (
                    self.env["website.page"]
                    .with_user(svc_uid)
                    .search(
                        [
                            ("user_websites_group_id", "in", group_ids),
                            "|",
                            ("is_published", "=", True),
                            ("website_published", "=", True),
                        ],
                        limit=5000,
                    )
                )
                if not pages:
                    break
                pages.with_context(mail_notrack=True).write(
                    {"is_published": False, "website_published": False}
                )
            while True:
                posts = (
                    self.env["blog.post"]
                    .with_user(svc_uid)
                    .search(
                        [
                            ("user_websites_group_id", "in", group_ids),
                            ("is_published", "=", True),
                        ],
                        limit=5000,
                    )
                )
                if not posts:
                    break
                posts.with_context(mail_notrack=True).write({"is_published": False})

        for group in self:
            group.is_suspended_from_websites = True
            # See res_users_moderation.py's identical call: _get_page_id_by_url()
            # also checks user_websites_group_id.is_suspended_from_websites at
            # cache-population time and must not keep serving a stale cached hit.
            notify_model_invalidation(self.env, "website.page")
            mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
                "user_websites.user_websites_service_account"
            )
            group.with_user(mail_svc).message_post(
                body=_(
                    "🚨 **AUTOMATED ACTION:** The system suspended this group for accumulating 3 or more violation strikes and unpublished their shared content."
                ),
                subtype_xmlid="mail.mt_note",
            )

    # [@ANCHOR: user_websites:COMM_action_pardon_group_websites]
    def action_pardon_group_websites(self):
        """Resets strikes and lifts the suspension (Does NOT automatically republish content)."""
        for group in self:
            group.violation_strike_count = 0
            group.is_suspended_from_websites = False
            notify_model_invalidation(self.env, "website.page")
            mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
                "user_websites.user_websites_service_account"
            )
            group.with_user(mail_svc).message_post(
                body=_(
                    "✅ **MODERATION ACTION:** You pardoned this group. The system lifted their suspension and reset their strike count to 0. (Note: Previously unpublished content remains unpublished until manually restored)."
                ),
                subtype_xmlid="mail.mt_note",
            )
