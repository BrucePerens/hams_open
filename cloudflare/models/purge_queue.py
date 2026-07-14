# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0.
import logging
import time
from odoo import models, fields, api
from ..utils.cloudflare_api import purge_everything, purge_urls, purge_tags

_logger = logging.getLogger(__name__)


class CloudflarePurgeQueue(models.Model):
    _name = "cloudflare.purge.queue"
    _description = "Cloudflare Cache Purge Queue"
    name = fields.Char(string="Name", default=lambda self: self._description)

    target_item = fields.Char(string="Target Payload", required=False)
    purge_type = fields.Selection(
        [("url", "URL"), ("tag", "Cache-Tag"), ("everything", "Everything")],
        default="url",
        required=True,
    )
    state = fields.Selection(
        [("pending", "Pending"), ("failed", "Failed")], default="pending", index=True
    )
    website_id = fields.Many2one("website", string="Website", ondelete="cascade")

    @api.model
    def enqueue_urls_batch(self, purge_map):
        if not purge_map:
            return

        website_ids = list(purge_map.keys())
        websites = self.env["website"].browse(website_ids)
        website_dict = {w.id: w for w in websites}

        default_base_url = (
            self.env["zero_sudo.security.utils"]
            ._get_system_param("web.base.url", "https://odoo")
            .rstrip("/")
        )

        create_vals = []
        for wid, urls in purge_map.items():
            website = website_dict.get(wid)
            base_url = (
                website.domain.rstrip("/")
                if website and website.domain
                else default_base_url
            )
            for u in urls:
                if not u:
                    continue
                full_url = f"{base_url}{u}" if str(u).startswith("/") else u
                create_vals.append(
                    {
                        "target_item": full_url,
                        "purge_type": "url",
                        "website_id": wid if wid else False,
                    }
                )

        if create_vals:
            self.env["cloudflare.purge.queue"].create(create_vals)

    @api.model
    def enqueue_urls(self, urls, website_id=None):
        # [@ANCHOR: COMM_enqueue_urls_base_url]

        # Verified by [@ANCHOR: COMM_test_purge_queue_base_url_sudo]
        if not website_id:
            website_id = self.env["cloudflare.utils"].get_current_website_id()
        self.enqueue_urls_batch({website_id: urls})

    @api.model
    def enqueue_tags(self, tags, website_id=None):
        # [@ANCHOR: COMM_cf_enqueue_tags_api]

        # Verified by [@ANCHOR: COMM_test_purge_tags_api]

        # Verified by [@ANCHOR: test_purge_queue_tags_processing]
        if not website_id:
            website_id = self.env["cloudflare.utils"].get_current_website_id()

        create_vals = [
            {"target_item": t, "purge_type": "tag", "website_id": website_id}
            for t in tags
            if t
        ]
        if create_vals:
            self.env["cloudflare.purge.queue"].create(create_vals)

    @api.model
    def enqueue_everything(self, website_ids=None):
        # [@ANCHOR: COMM_cf_enqueue_everything]
        if not website_ids:
            website_ids = [self.env["cloudflare.utils"].get_current_website_id()]

        if not isinstance(website_ids, (list, tuple, set)):
            website_ids = [website_ids]

        create_vals = [
            {"purge_type": "everything", "website_id": wid}
            for wid in website_ids
            if wid
        ]
        if create_vals:
            self.env["cloudflare.purge.queue"].create(create_vals)

    @api.model
    def process_queue(self):
        # [@ANCHOR: COMM_cf_process_queue_logic]
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_purge"
        )
        self = self.with_user(svc_uid)

        limit = 30
        max_batches = 10
        batches_processed = 0

        while batches_processed < max_batches:
            # We must process in batches to avoid timing out and to respect Cloudflare rate limits.
            # However, we also need to group by website/zone because each zone requires a different API call.
            records = self.env["cloudflare.purge.queue"].search(
                [("state", "=", "pending")], order="website_id, id", limit=limit
            )
            if not records:
                break

            # Process strictly by website to prevent credential mixing
            first_website = records[0].website_id
            batch_records = records.filtered(lambda r: r.website_id == first_website)

            if first_website:
                token, zone_id = first_website._get_cloudflare_credentials()
            else:
                # If there's no website_id, we fail fast.
                token = None
                zone_id = None

            everything_records = batch_records.filtered(
                lambda r: r.purge_type == "everything"
            )

            success = True

            if not token or not zone_id:
                # Missing credentials, immediately fail the batch to prevent infinite loops
                success = False
                batch_records.write({"state": "failed"})
            else:
                # If we are purging everything for this website, we can drop all other pending records for it.
                if everything_records:
                    if not purge_everything(token, zone_id):
                        success = False
                        everything_records.write({"state": "failed"})
                    else:
                        everything_records.unlink()
                        # Optimization: Clear all other pending records for this website since we just wiped everything
                        self.env.cr.execute(  # audit-ignore-sql: Tested by [@ANCHOR: COMM_test_queue_batching_and_rate_limiting]  # fmt: skip
                            "DELETE FROM cloudflare_purge_queue WHERE website_id = %s AND state = 'pending'",
                            (first_website.id,),
                        )

                # Refresh batch_records by filtering out non-existent ones before further processing
                batch_records = batch_records.exists()

                url_records = batch_records.filtered(lambda r: r.purge_type == "url")
                tag_records = batch_records.filtered(lambda r: r.purge_type == "tag")

                if url_records:
                    if not purge_urls(
                        url_records.mapped("target_item"), token, zone_id
                    ):
                        success = False
                        url_records.write({"state": "failed"})
                    else:
                        url_records.unlink()

                if tag_records:
                    if not purge_tags(
                        tag_records.mapped("target_item"), token, zone_id
                    ):
                        success = False
                        tag_records.write({"state": "failed"})
                    else:
                        tag_records.unlink()

            if not success:
                self.env.cr.commit()

            batches_processed += 1
            self.env.cr.commit()
            time.sleep(0.1)  # Drop DB locks and respect rate limit # audit-ignore-sleep

        if batches_processed >= max_batches:
            cron = self.env.ref(
                "cloudflare.ir_cron_process_cf_purge_queue", raise_if_not_found=False
            )
            if cron:
                cron._trigger()
