# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""
This file defines the Odoo model for User Websites Groups.
"""

from odoo import models, fields, api, _
from odoo.exceptions import ValidationError
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


def _async_unpublish_group_content(db_name, group_ids):
    """Unpublishes group content in the background to prevent transaction lock exhaustion."""
    registry = Registry(db_name)
    cr = registry.cursor()
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
                company_env = env_svc.with_company(company_id)
                while True:
                    pages = company_env["website.page"].search(
                        [
                            ("user_websites_group_id", "in", comp_group_ids),
                            ("website_published", "=", True),
                        ],
                        limit=5000,
                    )
                    if not pages:
                        break
                    pages.with_context(mail_notrack=True).write(
                        {"website_published": False}
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
                _unpublish_for_company(company_id, comp_group_ids)

        finally:
            env.cr.rollback()
    except psycopg2.Error as e:  # audit-ignore-catch-all
        _logger.error("Async unpublish group content failed: %s", e)
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
                    group_vals["privilege_id"] = category_id

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

    def write(self, vals):
        old_slugs = {}
        if "website_slug" in vals:
            old_slugs = {
                group.id: group.website_slug for group in self if group.website_slug
            }

        try:
            with self.env.cr.savepoint():
                result = super(UserWebsitesGroup, self).write(vals)
        except IntegrityError:
            raise ValidationError(_("The Group Website Slug must be unique and valid."))

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

    def action_suspend_group_websites(self):
        """Forcefully unpublishes all group content and flags them as suspended."""
        group_ids = self.ids
        is_test = vars(self.env.registry).get("test_cr") is not None

        if not is_test:
            db_name = self.env.cr.dbname
            BACKGROUND_EXECUTOR.submit(
                _async_unpublish_group_content, db_name, group_ids
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
            mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
                "user_websites.user_websites_service_account"
            )
            group.with_user(mail_svc).message_post(
                body=_(
                    "🚨 **AUTOMATED ACTION:** The system suspended this group for accumulating 3 or more violation strikes and unpublished their shared content."
                ),
                subtype_xmlid="mail.mt_note",
            )

    def action_pardon_group_websites(self):
        """Resets strikes and lifts the suspension (Does NOT automatically republish content)."""
        for group in self:
            group.violation_strike_count = 0
            group.is_suspended_from_websites = False
            mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
                "user_websites.user_websites_service_account"
            )
            group.with_user(mail_svc).message_post(
                body=_(
                    "✅ **MODERATION ACTION:** You pardoned this group. The system lifted their suspension and reset their strike count to 0. (Note: Previously unpublished content remains unpublished until manually restored)."
                ),
                subtype_xmlid="mail.mt_note",
            )
