# -*- coding: utf-8 -*-
from odoo import models


class WebsitePage(models.Model):
    _inherit = "website.page"

    def write(self, vals):
        # Cache info for purging after super().write()
        # Grouped by website_id to reduce ORM overhead
        purge_map = {}  # {website_id: [urls]}
        all_website_ids = None
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_purge"
        )

        any_missing_website = any(not p.website_id for p in self)
        if any_missing_website:
            all_website_ids = (
                self.env["website"].with_user(svc_uid).search([], limit=1000).ids
            )

        for page in self:
            if page.url:
                wids = (
                    [page.website_id.id]
                    if page.website_id
                    else (all_website_ids if any_missing_website else [])
                )
                for wid in wids:
                    purge_map.setdefault(wid, []).append(page.url)

        res = super().write(vals)

        # Zero-Sudo Execution: Add to queue securely
        # Total Context Annihilation to prevent ORM KeyError: 'record'
        sterile_env = self.env(context={})
        QueueModel = sterile_env["cloudflare.purge.queue"].with_user(svc_uid)
        for wid, urls in purge_map.items():
            QueueModel.enqueue_urls(urls, website_id=wid)

        return res

    def unlink(self):
        # Cache info for purging before super().unlink()
        purge_map = {}
        all_website_ids = None
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_purge"
        )

        any_missing_website = any(not p.website_id for p in self)
        if any_missing_website:
            all_website_ids = (
                self.env["website"].with_user(svc_uid).search([], limit=1000).ids
            )

        for page in self:
            if page.url:
                wids = (
                    [page.website_id.id]
                    if page.website_id
                    else (all_website_ids if any_missing_website else [])
                )
                for wid in wids:
                    purge_map.setdefault(wid, []).append(page.url)

        res = super().unlink()

        sterile_env = self.env(context={})
        QueueModel = sterile_env["cloudflare.purge.queue"].with_user(svc_uid)
        for wid, urls in purge_map.items():
            QueueModel.enqueue_urls(urls, website_id=wid)

        return res


class BlogPost(models.Model):
    _inherit = "blog.post"

    def write(self, vals):
        purge_map = {}
        all_website_ids = None
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_purge"
        )

        any_missing_website = any(not p.website_id for p in self)
        if any_missing_website:
            all_website_ids = (
                self.env["website"].with_user(svc_uid).search([], limit=1000).ids
            )

        for post in self:
            if post.website_url:
                wids = (
                    [post.website_id.id]
                    if post.website_id
                    else (all_website_ids if any_missing_website else [])
                )
                for wid in wids:
                    purge_map.setdefault(wid, []).append(post.website_url)

        res = super().write(vals)

        if purge_map:
            sterile_env = self.env(context={})
            QueueModel = sterile_env["cloudflare.purge.queue"].with_user(svc_uid)
            for wid, urls in purge_map.items():
                QueueModel.enqueue_urls(urls, website_id=wid)

        return res


class WebsiteMenu(models.Model):
    _inherit = "website.menu"

    def write(self, vals):
        # Menu changes usually mean the header/footer changed.
        # We trigger a global purge for the website(s) affected.
        website_ids = self.mapped("website_id").ids
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_purge"
        )

        if any(not m.website_id for m in self):
            website_ids = (
                self.env["website"].with_user(svc_uid).search([], limit=1000).ids
            )

        res = super().write(vals)

        sterile_env = self.env(context={})
        QueueModel = sterile_env["cloudflare.purge.queue"].with_user(svc_uid)
        for wid in set(website_ids):
            # Menu changes affect HTML content globally. We must purge everything for the website.
            QueueModel.enqueue_everything(website_id=wid)

        return res

    def unlink(self):
        website_ids = self.mapped("website_id").ids
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_purge"
        )

        if any(not m.website_id for m in self):
            website_ids = (
                self.env["website"].with_user(svc_uid).search([], limit=1000).ids
            )

        res = super().unlink()

        sterile_env = self.env(context={})
        QueueModel = sterile_env["cloudflare.purge.queue"].with_user(svc_uid)
        for wid in set(website_ids):
            # Menu changes affect HTML content globally. We must purge everything for the website.
            QueueModel.enqueue_everything(website_id=wid)

        return res


class ProductTemplate(models.Model):
    _inherit = "product.template"

    def write(self, vals):
        purge_map = {}
        all_website_ids = None
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_purge"
        )

        any_missing_website = any(not p.website_id for p in self)
        if any_missing_website:
            all_website_ids = (
                self.env["website"].with_user(svc_uid).search([], limit=1000).ids
            )

        for product in self:
            if product.website_url:
                # website_id on product.template is from website_sale
                wids = (
                    [product.website_id.id]
                    if product.website_id
                    else (all_website_ids if any_missing_website else [])
                )
                for wid in wids:
                    purge_map.setdefault(wid, []).append(product.website_url)

        res = super().write(vals)

        if purge_map:
            sterile_env = self.env(context={})
            QueueModel = sterile_env["cloudflare.purge.queue"].with_user(svc_uid)
            for wid, urls in purge_map.items():
                QueueModel.enqueue_urls(urls, website_id=wid)

        return res
