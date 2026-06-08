# -*- coding: utf-8 -*-
import time
import os
import logging
from odoo import models, fields, api, tools
from ..utils.cloudflare_api import purge_everything, purge_urls, purge_tags

_logger = logging.getLogger(__name__)


class CloudflarePurgeQueue(models.Model):
    _name = "cloudflare.purge.queue"
    _description = "Cloudflare Cache Purge Queue"

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
    def enqueue_urls(self, urls, website_id=None):
        # [@ANCHOR: enqueue_urls_base_url]
        # Verified by [@ANCHOR: test_purge_queue_base_url_sudo]
        if not website_id:
            website_id = self.env["cloudflare.utils"].get_current_website_id()

        website = self.env["website"].browse(website_id)

        if website and website.domain:
            base_url = website.domain.rstrip("/")
        else:
            base_url = (
                self.env["zero_sudo.security.utils"]
                ._get_system_param("web.base.url", "https://odoo")
                .rstrip("/")
            )

        create_vals = []
        for u in urls:
            if not u:
                continue
            full_url = f"{base_url}{u}" if str(u).startswith("/") else u
            create_vals.append(
                {
                    "target_item": full_url,
                    "purge_type": "url",
                    "website_id": website.id if website else False,
                }
            )

        if create_vals:
            self.env["cloudflare.purge.queue"].create(create_vals)

    @api.model
    def enqueue_tags(self, tags, website_id=None):
        # [@ANCHOR: cf_enqueue_tags_api]
        # Verified by [@ANCHOR: test_purge_tags_api]
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
    def enqueue_everything(self, website_id=None):
        # [@ANCHOR: cf_enqueue_everything]
        if not website_id:
            website_id = self.env["cloudflare.utils"].get_current_website_id()

        self.env["cloudflare.purge.queue"].create(
            {"purge_type": "everything", "website_id": website_id}
        )

    @api.model
    def process_queue(self):
        # [@ANCHOR: cf_process_queue_logic]
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
                        self.env["cloudflare.purge.queue"].search(
                            [
                                ("website_id", "=", first_website.id),
                                ("state", "=", "pending"),
                            ],
                            limit=10000,
                        ).unlink()

                # Refresh batch_records by filtering out non-existent ones before further processing
                batch_records = batch_records.filtered(lambda r: r.exists())

                url_records = batch_records.filtered(lambda r: r.purge_type == "url")
                tag_records = batch_records.filtered(lambda r: r.purge_type == "tag")

                if success and url_records:
                    if not purge_urls(
                        url_records.mapped("target_item"), token, zone_id
                    ):
                        success = False
                        url_records.write({"state": "failed"})
                    else:
                        url_records.unlink()

                if success and tag_records:
                    if not purge_tags(
                        tag_records.mapped("target_item"), token, zone_id
                    ):
                        success = False
                        tag_records.write({"state": "failed"})
                    else:
                        tag_records.unlink()

            if not success:
                if not tools.config.get("test_enable"):
                    self.env.cr.commit()

            batches_processed += 1
            if not tools.config.get("test_enable"):
                self.env.cr.commit()

            if not os.environ.get("HAMS_DISABLE_SLEEPS"):
                time.sleep(0.5)  # audit-ignore-sleep: Rate limiting  # fmt: skip

        if batches_processed >= max_batches:
            cron = self.env.ref(
                "cloudflare.ir_cron_process_cf_purge_queue", raise_if_not_found=False
            )
            if cron:
                cron._trigger()
