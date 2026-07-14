# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0.
from odoo import models, fields


class CloudflarePurgeMixin(models.AbstractModel):
    _name = "cloudflare.purge.mixin"
    _description = "Cloudflare Purge Mixin"
    name = fields.Char(string="Name")

    def _enqueue_cloudflare_purge(self, url_field):
        purge_map = {}
        all_website_ids = None
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_purge"
        )

        any_missing_website = any(not r.website_id for r in self)
        if any_missing_website:
            all_website_ids = (
                self.env["website"].with_user(svc_uid).search([], limit=1000).ids
            )

        for rec in self:
            url = rec[url_field]
            if url:
                wid = rec.website_id
                wids = (
                    [wid.id]
                    if wid
                    else (all_website_ids if any_missing_website else [])
                )
                for w in wids:
                    purge_map.setdefault(w, []).append(url)

        if purge_map:
            QueueModel = self.env["cloudflare.purge.queue"].with_user(svc_uid)
            QueueModel.enqueue_urls_batch(purge_map)

    def _purge_cloudflare_menus(self):
        website_ids = self.mapped("website_id").ids
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_purge"
        )

        if any(not m.website_id for m in self):
            website_ids = (
                self.env["website"].with_user(svc_uid).search([], limit=1000).ids
            )

        QueueModel = self.env["cloudflare.purge.queue"].with_user(svc_uid)
        if website_ids:
            QueueModel.enqueue_everything(website_ids=list(set(website_ids)))
