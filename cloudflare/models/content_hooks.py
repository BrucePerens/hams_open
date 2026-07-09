# -*- coding: utf-8 -*-
from odoo import models


def _enqueue_purge_for_records(records, url_field):
    purge_map = {}
    all_website_ids = None
    svc_uid = records.env["zero_sudo.security.utils"]._get_service_uid(
        "cloudflare.user_cloudflare_purge"
    )

    any_missing_website = any(not r.website_id for r in records)
    if any_missing_website:
        all_website_ids = (
            records.env["website"].with_user(svc_uid).search([], limit=1000).ids
        )

    for rec in records:
        url = rec[url_field] if url_field in rec else False
        if url:
            wid = rec.website_id if "website_id" in rec else False
            wids = (
                [wid.id]
                if wid
                else (all_website_ids if any_missing_website else [])
            )
            for w in wids:
                purge_map.setdefault(w, []).append(url)

    if purge_map:
        sterile_env = records.env(context={})
        QueueModel = sterile_env["cloudflare.purge.queue"].with_user(svc_uid)
        QueueModel.enqueue_urls_batch(purge_map)


class WebsitePage(models.Model):
    _inherit = "website.page"

    def write(self, vals):
        _enqueue_purge_for_records(self, "url")
        return super().write(vals)

    def unlink(self):
        _enqueue_purge_for_records(self, "url")
        return super().unlink()


class BlogPost(models.Model):
    _inherit = "blog.post"

    def write(self, vals):
        _enqueue_purge_for_records(self, "website_url")
        return super().write(vals)


class WebsiteMenu(models.Model):
    _inherit = "website.menu"

    def write(self, vals):
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
        if website_ids:
            QueueModel.enqueue_everything(website_ids=list(set(website_ids)))

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
        if website_ids:
            QueueModel.enqueue_everything(website_ids=list(set(website_ids)))

        return res


class ProductTemplate(models.Model):
    _inherit = "product.template"

    def write(self, vals):
        _enqueue_purge_for_records(self, "website_url")
        return super().write(vals)
