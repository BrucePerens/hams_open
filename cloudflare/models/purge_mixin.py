# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
from odoo import models, fields


# [@ANCHOR: cloudflare_purge_mixin_cloudflare_purge_mixin]
class CloudflarePurgeMixin(models.AbstractModel):
    _name = "cloudflare.purge.mixin"
    _description = "Cloudflare Purge Mixin"
    name = fields.Char(string="Name")

    # [@ANCHOR: cloudflare:COMM_enqueue_cloudflare_purge]
    def _enqueue_cloudflare_purge(self, url_field):
        purge_map = {}
        all_website_ids = None
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_purge"
        )
        # A website_id=False
        # record here is genuinely served on EVERY website in the whole
        # install, not just the acting user's own company -- confirmed
        # against user_websites/models/website_page.py's own
        # _get_page_id_by_url, whose lookup domain is
        # `[('website_id', '=', False), ('website_id', '=', website_id)]`
        # with no company filter at all, and matches stock Odoo's own
        # website.page multi-website domain (website.py:
        # `Domain('website_id', 'in', [False, *self.ids])`). The fallback
        # search below is therefore deliberately NOT scoped to
        # `self.env.companies`: fanning a purge out to every website is not a
        # cross-tenant amplification problem, because such content really is
        # rendered on every tenant's site, so purging every website IS the
        # correct cache-coherence behavior, and a company-scoped search would
        # leave every OTHER tenant serving stale content after a legitimate
        # edit to global content. The separate question of who can actually
        # create/edit a website_id=False record at all is an access-control
        # property of website.page/website.menu/etc. themselves, not of this
        # purge queue; see this function's own claim file for the full
        # writeup. Company-scoping the search would not tighten that access
        # control; it would only leave other tenants' edge caches stale after
        # a legitimate edit to global content. Company-scoping the search would not tighten that access
        # control; it would only leave other tenants' edge caches stale after
        # a legitimate edit to global content.
        any_missing_website = any(not r.website_id for r in self)
        if any_missing_website:
            all_website_ids = (
                self.env["website"].with_user(svc_uid).search([], limit=1000).ids
            )

        for rec in self:
            url = rec[url_field]
            if url:
                wid = rec.website_id.id if 'website_id' in rec._fields and rec.website_id else False
                wids = (
                    [wid]
                    if wid
                    else (all_website_ids if any_missing_website else [])
                )
                for w in wids:
                    purge_map.setdefault(w, []).append(url)

        if purge_map:
            QueueModel = self.env["cloudflare.purge.queue"].with_user(svc_uid)
            QueueModel.enqueue_urls_batch(purge_map)

    # [@ANCHOR: cloudflare:COMM_purge_cloudflare_menus]
    def _purge_cloudflare_menus(self):
        website_ids = self.mapped("website_id").ids
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_purge"
        )
        # See _enqueue_cloudflare_purge's own comment above -- fanning out to
        # every website when a record has no website_id is correct-by-design
        # here (website_id=False content is genuinely served on every
        # tenant's site), not a cross-tenant scoping bug. Left unscoped
        # deliberately: narrowing it to `self.env.companies` would leave other
        # tenants' edge caches stale.
        if any(not m.website_id for m in self):
            website_ids = (
                self.env["website"].with_user(svc_uid).search([], limit=1000).ids
            )

        QueueModel = self.env["cloudflare.purge.queue"].with_user(svc_uid)
        if website_ids:
            QueueModel.enqueue_everything(website_ids=list(set(website_ids)))
