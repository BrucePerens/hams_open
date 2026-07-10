# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
from odoo import models, fields, api, _
from odoo.exceptions import AccessError
import time
import hashlib
import hmac
import logging
from markupsafe import Markup

_logger = logging.getLogger(__name__)


class BlogPost(models.Model):
    _name = "blog.post"
    name = fields.Char(string="Name", default=lambda self: self._description)
    _inherit = ["blog.post", "user_websites.owned.mixin"]

    view_count = fields.Integer(
        string="View Count", default=0, help="Privacy-friendly tracking of post views."
    )

    _name_owner_uniq = models.Constraint("UNIQUE(name, owner_user_id, user_websites_group_id)", "You already have a blog post with this exact title!")

    def _invalidate_cloudflare_cache(self):
        """Purge the global Cache-Tag at the edge."""
        if "cloudflare.purge.queue" not in self.env:
            return

        # ADR 0078: Pre-fetch related fields to prevent N+1 queries in the loop
        self.mapped("owner_user_id.website_slug")
        self.mapped("user_websites_group_id.website_slug")

        tags = set()
        for rec in self:
            if rec.owner_user_id and rec.owner_user_id.website_slug:
                tags.add(f"site-{rec.owner_user_id.website_slug}")
            elif rec.user_websites_group_id and rec.user_websites_group_id.website_slug:
                tags.add(f"site-{rec.user_websites_group_id.website_slug}")
        if tags:
            try:
                svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                    "cloudflare.user_cloudflare_purge"
                )
                self.env["cloudflare.purge.queue"].with_user(svc_uid).enqueue_tags(
                    list(tags)
                )
            except AccessError as e:
                if "Service Account" in str(e):
                    logging.getLogger(__name__).debug("Cloudflare purge skipped: %s", e)
                else:
                    logging.getLogger(__name__).exception(
                        "Access error during Cloudflare purge"
                    )
            except Exception:  # audit-ignore-catch-all
                logging.getLogger(__name__).exception(
                    "Fatal error during Cloudflare purge"
                )

    def _get_blog_urls(self):
        """Helper method to construct the blog index URLs for Cloudflare cache invalidation."""
        urls = set()
        # ADR 0078: Pre-fetch related fields
        self.mapped("owner_user_id.website_slug")
        self.mapped("user_websites_group_id.website_slug")

        for post in self:
            if post.owner_user_id and post.owner_user_id.website_slug:
                urls.add(f"/{post.owner_user_id.website_slug}/blog")
            elif (
                post.user_websites_group_id and post.user_websites_group_id.website_slug
            ):
                urls.add(f"/{post.user_websites_group_id.website_slug}/blog")
        return list(urls)

    @api.model_create_multi
    def create(self, vals_list):
        # Tested by [@ANCHOR: user_websites:test_group_blog_post_creation]
        # Tested by [@ANCHOR: user_websites:test_tour_create_blog]
        self._check_proxy_ownership_create(vals_list)
        if not (
            self.env.su
            or self.env.user.has_group("base.group_system")
            or self.env.user.has_group(
                "user_websites.group_user_websites_administrator"
            )
        ):
            allowed = {
                "name",
                "content",
                "is_published",
                "owner_user_id",
                "user_websites_group_id",
                "blog_id",
                "website_id",
                "view_count",
                "website_meta_title",
                "website_meta_description",
                "website_meta_keywords",
                "website_meta_og_img",
                "seo_name",
            }
            for vals in vals_list:
                for k in list(vals.keys()):
                    if k not in allowed:
                        del vals[k]
        try:
            svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                "user_websites.user_websites_service_account"
            )
            # ADR-0001: All service account mutations must include appropriate context
            self_svc = self.with_user(svc_uid).with_context(mail_notrack=True)
            posts = super(BlogPost, self_svc).create(vals_list)
        except AccessError as e:
            if "not found" in str(e):
                posts = super(BlogPost, self).create(vals_list)
            else:
                raise

        utils = self.env["zero_sudo.security.utils"]
        for url in posts._get_blog_urls():
            utils._notify_cache_invalidation("blog.post", url)

        posts._invalidate_cloudflare_cache()
        return posts

    def check_access(self, operation):
        """
        Proactively catch write/unlink access violations to prevent ir.rule INFO log spam
        when the frontend evaluates edit capabilities.
        """
        if operation in ("write", "unlink") and not self.env.su and self:
            if self.env.user.has_group(
                "user_websites.group_user_websites_user"
            ) and not self.env.user.has_group(
                "user_websites.group_user_websites_administrator"
            ):
                user_id = self.env.user.id

                # ADR-0022: Pre-fetch group memberships to prevent N+1 lazy-load queries in the loop
                group_ids = self.mapped("user_websites_group_id").ids
                member_map = {}
                if group_ids:
                    svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                        "user_websites.user_websites_service_account"
                    )
                    groups = (
                        self.env["user.websites.group"]
                        .with_user(svc_uid)
                        .browse(group_ids)
                    )
                    for g in groups:
                        member_map[g.id] = set(g.member_ids.ids)

                for post in self:
                    is_owner = post.owner_user_id.id == user_id
                    is_group_member = (
                        post.user_websites_group_id
                        and user_id
                        in member_map.get(post.user_websites_group_id.id, set())
                    )
                    if not is_owner and not is_group_member:

                        raise AccessError(
                            _(
                                "Access Denied: You do not have permission to modify this post."
                            )
                        )
        return super(BlogPost, self).check_access(operation)

    def write(self, vals):
        self.check_access("write")
        self._check_proxy_ownership_write(vals)

        if not (
            self.env.su
            or self.env.user.has_group("base.group_system")
            or self.env.user.has_group(
                "user_websites.group_user_websites_administrator"
            )
        ):
            allowed = {
                "name",
                "content",
                "is_published",
                "owner_user_id",
                "user_websites_group_id",
                "blog_id",
                "website_id",
                "view_count",
                "website_meta_title",
                "website_meta_description",
                "website_meta_keywords",
                "website_meta_og_img",
                "seo_name",
            }
            for k in list(vals.keys()):
                if k not in allowed:
                    del vals[k]

        urls_to_invalidate = self._get_blog_urls()

        try:
            svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                "user_websites.user_websites_service_account"
            )
            # ADR-0001: All service account mutations must include appropriate context
            self_svc = self.with_user(svc_uid).with_context(mail_notrack=True)
            res = super(BlogPost, self_svc).write(vals)
        except AccessError as e:
            if "not found" in str(e):
                res = super(BlogPost, self).write(vals)
            else:
                raise

        if "is_published" in vals or "name" in vals or "content" in vals:
            new_urls = self._get_blog_urls()
            all_urls = list(set(urls_to_invalidate + new_urls))
            utils = self.env["zero_sudo.security.utils"]
            if all_urls:
                utils._notify_cache_invalidation("blog.post", all_urls)

        self._invalidate_cloudflare_cache()
        return res

    def unlink(self):
        self.check_access("unlink")

        urls_to_invalidate = self._get_blog_urls()
        self._invalidate_cloudflare_cache()

        try:
            svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                "user_websites.user_websites_service_account"
            )
            # ADR-0001: All service account mutations must include appropriate context
            self_svc = self.with_user(svc_uid).with_context(mail_notrack=True)
            res = super(BlogPost, self_svc).unlink()
        except AccessError as e:
            if "not found" in str(e):
                res = super(BlogPost, self).unlink()
            else:
                raise

        utils = self.env["zero_sudo.security.utils"]
        if urls_to_invalidate:
            utils._notify_cache_invalidation("blog.post", urls_to_invalidate)

        return res

    @api.model
    def send_weekly_digest(self):
        # [@ANCHOR: send_weekly_digest]
        # Verified by [@ANCHOR: test_weekly_digest_secret]
        # Verified by [@ANCHOR: test_weekly_digest_mail_template]
        # Tested by [@ANCHOR: user_websites:test_subscribe_to_site]
        # Tested by [@ANCHOR: user_websites:test_subscription_creation]
        """
        Cron job method to send a weekly email digest.
        """
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_websites_service_account"
        )

        last_processed_id = int(
            self.env["zero_sudo.security.utils"]._get_system_param(
                "user_websites.last_digest_id", "0"
            )
            or "0"
        )

        digests = (
            self.env["user_websites.weekly_digest_view"]
            .with_user(svc_uid)
            .search([("id", ">", last_processed_id)], order="id asc", limit=50)
        )

        if not digests:
            self.env["ir.config_parameter"].with_user(svc_uid).set_param(
                "user_websites.last_digest_id", "0"
            )
            return

        template = self.env.ref(
            "user_websites.email_template_weekly_digest", raise_if_not_found=False
        )
        if not template:
            return

        base_url = self.env["zero_sudo.security.utils"]._get_system_param(
            "web.base.url"
        )
        db_secret = self.env["zero_sudo.security.utils"]._get_crypto_secret()
        if not db_secret:
            _logger.error(
                "Security Alert: Crypto secret is not configured. Weekly digest tokens cannot be generated."
            )
            return

        for digest in digests:
            partner = digest.partner_id
            if not partner or not partner.email:
                continue

            timestamp = int(time.time())
            message = f"{digest.owner_model}-{digest.owner_record_id}-{partner.id}-{timestamp}".encode(
                "utf-8"
            )
            token = hmac.new(
                db_secret.encode("utf-8"), message, hashlib.sha256
            ).hexdigest()
            unsub_url = f"{base_url}/website/unsubscribe/{digest.owner_model}/{digest.owner_record_id}/{partner.id}/{timestamp}/{token}"

            headers = {
                "List-Unsubscribe": f"<<{unsub_url}>>",
                "List-Unsubscribe-Post": "List-Unsubscribe=One-Click",
            }

            post_ids = [int(pid) for pid in digest.post_ids_string.split(",") if pid]
            posts = self.env["blog.post"].with_user(svc_uid).browse(post_ids)
            post_links_html = "".join(
                f"<li><a href='{base_url}{p.website_url}'>{p.name}</a></li>"
                for p in posts
            )

            ctx = {
                "author_name": digest.author_name,
                "post_links": Markup(post_links_html),
                "email_to": partner.email,
                "unsub_url": unsub_url,
            }
            email_vals = {
                "headers": repr(headers),
                "recipient_ids": [(4, partner.id)],
                "email_to": partner.email,
            }
            mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
                "zero_sudo.mail_service_internal"
            )
            template.with_user(mail_svc).with_context(**ctx).send_mail(digest.first_post_id, force_send=False, email_values=email_vals)  # audit-ignore-mail: Tested by [@ANCHOR: test_weekly_digest_mail_template]  # fmt: skip

        if len(digests) == 50:
            self.env["ir.config_parameter"].with_user(svc_uid).set_param(
                "user_websites.last_digest_id", str(digests[-1].id)
            )
            self.env.ref("user_websites.ir_cron_send_weekly_digest")._trigger()
        else:
            self.env["ir.config_parameter"].with_user(svc_uid).set_param(
                "user_websites.last_digest_id", "0"
            )
