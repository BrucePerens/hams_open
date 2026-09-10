# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
from odoo import models, fields


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
        # Bug-hunt note (review_tier 1, 2026-09-09): a website_id=False
        # record here is genuinely served on EVERY website in the whole
        # install, not just the acting user's own company -- confirmed
        # against user_websites/models/website_page.py's own
        # _get_page_id_by_url, whose lookup domain is
        # `[('website_id', '=', False), ('website_id', '=', website_id)]`
        # with no company filter at all, and matches stock Odoo's own
        # website.page multi-website domain (website.py:
        # `Domain('website_id', 'in', [False, *self.ids])`). An earlier
        # version of this fix scoped the fallback search below to
        # `self.env.companies` on the theory that fanning a purge out to
        # every website was a cross-tenant amplification bug -- that was
        # wrong: since such content really is rendered on every tenant's
        # site, purging every website IS the correct cache-coherence
        # behavior, and the company-scoped version would have left every
        # OTHER tenant serving stale content after a legitimate edit to
        # global content. Reverted to the original unscoped search. The
        # real, unresolved question this dispatch's callout was reaching
        # for is upstream of this file: who can actually create/edit a
        # website_id=False record at all (an access-control property of
        # website.page/website.menu/etc. themselves, not of this purge
        # queue) -- not independently verified in this pass; see this
        # function's own claim file for the full writeup.
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
        # Bug-hunt note (review_tier 1, 2026-09-09): see
        # _enqueue_cloudflare_purge's own comment above -- fanning out to
        # every website when a record has no website_id is correct-by-design
        # here (website_id=False content is genuinely served on every
        # tenant's site), not a cross-tenant scoping bug. Left unscoped
        # deliberately, after an earlier version of this fix wrongly
        # narrowed it and was reverted.
        if any(not m.website_id for m in self):
            website_ids = (
                self.env["website"].with_user(svc_uid).search([], limit=1000).ids
            )

        QueueModel = self.env["cloudflare.purge.queue"].with_user(svc_uid)
        if website_ids:
            QueueModel.enqueue_everything(website_ids=list(set(website_ids)))
