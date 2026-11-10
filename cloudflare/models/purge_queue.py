# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
import logging
import time
from odoo import models, fields, api
from ..utils.cloudflare_api import purge_everything, purge_urls, purge_tags

_logger = logging.getLogger(__name__)

# Source name of the pager incident that says purges are not reaching Cloudflare.
PURGE_INCIDENT_SOURCE = "Cloudflare purge queue"
# A purge that failed on a real API error is tried again once, this long after it failed.
RETRY_FAILED_AFTER_HOURS = 1


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
    # [@ANCHOR: cloudflare:COMM_enqueue_urls_batch]
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

        # Resolve every candidate (website, full_url) pair first, then look up ONLY those
        # targets among the pending rows. The previous "search the first 1000 pending rows"
        # window made any URL beyond it invisible to the dedup check on a backlogged website
        # (todo 5b71945e), so the lookup is now keyed by the candidates and needs no cap.
        candidates = {}
        for wid, urls in purge_map.items():
            website = website_dict.get(wid)
            base_url = (
                website.domain.rstrip("/")
                if website and website.domain
                else default_base_url
            )
            for u in set(urls):
                if not u:
                    continue
                full_url = f"{base_url}{u}" if str(u).startswith("/") else u
                candidates.setdefault(wid if wid else False, set()).add(full_url)

        all_urls = sorted({u for urls in candidates.values() for u in urls})
        existing = self.env["cloudflare.purge.queue"].search(
            [
                ("website_id", "in", list(candidates)),
                ("state", "=", "pending"),
                ("purge_type", "=", "url"),
                ("target_item", "in", all_urls),
            ]
        )
        already = {
            (r.website_id.id if r.website_id else False, r.target_item)
            for r in existing
        }

        create_vals = []
        for wid, full_urls in candidates.items():
            for full_url in sorted(full_urls):
                if (wid, full_url) not in already:
                    create_vals.append(
                        {
                            "target_item": full_url,
                            "purge_type": "url",
                            "website_id": wid,
                        }
                    )

        if create_vals:
            self.env["cloudflare.purge.queue"].create(create_vals)

    @api.model
    def enqueue_urls(self, urls, website_id=None):
        # [@ANCHOR: COMM_enqueue_urls_base_url]

        # # Verified by [@ANCHOR: COMM_test_purge_queue_base_url_sudo]
        if not website_id:
            website_id = self.env["cloudflare.utils"].get_current_website_id()
        self.enqueue_urls_batch({website_id: urls})

    @api.model
    def enqueue_tags(self, tags, website_id=None):
        # [@ANCHOR: COMM_cf_enqueue_tags_api]

        # # Verified by [@ANCHOR: COMM_test_purge_tags_api]

        # # Verified by [@ANCHOR: test_purge_queue_tags_processing]
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
    def _pager_incidents(self):
        # [@ANCHOR: cloudflare:COMM_purge_pager_incidents]
        """The pager.incident model as the pager service user, or None when pager_duty is not installed.

        A tenant instance can run this module without pager_duty; there the log line is the only signal.
        """
        if "pager.incident" not in self.env:
            return None
        return self.env["zero_sudo.security.utils"]._get_service_env(
            "pager_duty.user_pager_service_internal"
        )["pager.incident"]

    @api.model
    def _report_purge_failure(self, website, description, severity):
        # [@ANCHOR: cloudflare:COMM_report_purge_failure]
        """Open (or add an occurrence to) the pager incident saying this website's purges are not reaching Cloudflare."""
        incidents = self._pager_incidents()
        if incidents is None:
            return False
        return incidents.report_incident(
            {
                "source": PURGE_INCIDENT_SOURCE,
                "severity": severity,
                "description": f"{website.name} (website {website.id}): {description}",
                "website_id": website.id,
            }
        )

    @api.model
    def _resolve_purge_failure(self, website):
        # [@ANCHOR: cloudflare:COMM_resolve_purge_failure]
        """Close the purge incident for this website after a batch of its purges succeeded."""
        incidents = self._pager_incidents()
        if incidents is None:
            return False
        return incidents.auto_resolve_incidents(PURGE_INCIDENT_SOURCE, website_id=website.id)

    @api.model
    def requeue_failed(self):
        # [@ANCHOR: cloudflare:COMM_requeue_failed_purges]
        """Make one more attempt at purges that failed on an API error at least an hour ago.

        Only for websites that have credentials now; a row that fails again goes back to failed and the
        pager incident stays open. Returns the number of rows set back to pending.
        """
        cutoff = fields.Datetime.subtract(fields.Datetime.now(), hours=RETRY_FAILED_AFTER_HOURS)
        failed = self.search([("state", "=", "failed"), ("write_date", "<=", cutoff)], limit=1000)
        requeue = self.browse()
        for website in failed.mapped("website_id"):
            token, zone_id = website._get_cloudflare_credentials()
            if token and zone_id:
                requeue |= failed.filtered(lambda r: r.website_id == website)
        requeue.write({"state": "pending"})
        return len(requeue)

    @api.private
    @api.model
    def process_queue(self):
        # [@ANCHOR: COMM_cf_process_queue_logic]
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_purge"
        )
        self = self.with_user(svc_uid)

        self.requeue_failed()

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
            failure_reason = "a Cloudflare API call failed; the purges are marked failed and retried once an hour"
            failure_severity = "high"

            if not token or not zone_id:
                # Missing credentials: these purges can never be sent. Drop them (a growing pile of failed rows
                # nobody reads is what 7,014 rows on 2026-10-29 was) and raise a pager incident instead.
                success = False
                failure_reason = (
                    "no Cloudflare API token or Zone ID is configured, so cache purges are being dropped; "
                    "set them in Settings > Cloudflare for this website"
                )
                failure_severity = "medium"
                # Log the failure: failing the batch with no signal anywhere
                # would leave an admin seeing only a growing pile of "failed"
                # purge.queue rows with no indication of *why* (no website
                # resolved at all, or that website's own credentials were
                # never configured). Log it so it's diagnosable without
                # having to reverse-engineer the cause from queue state.
                _logger.warning(
                    "Cloudflare purge queue: failing %d record(s) for website "
                    "id=%s -- missing API token or zone ID.",
                    len(batch_records),
                    first_website.id if first_website else False,
                )
                batch_records.unlink()
            else:
                # If we are purging everything for this website, we can drop all other pending records for it.
                if everything_records:
                    # Only rows that existed before the API call are covered by it: a purge queued
                    # while the call is in flight is for content that may be newer than the wipe.
                    self.env.cr.execute(  # audit-ignore-sql: # Tested by [@ANCHOR: cf_process_queue_logic]  # fmt: skip
                        "SELECT COALESCE(max(id), 0) FROM cloudflare_purge_queue "
                        "WHERE website_id = %s AND state = 'pending'",
                        (first_website.id,),
                    )
                    covered_up_to_id = self.env.cr.fetchone()[0]
                    if not purge_everything(token, zone_id):
                        success = False
                        everything_records.write({"state": "failed"})
                    else:
                        # # Verified by [@ANCHOR: COMM_test_queue_batching_and_rate_limiting]
                        everything_records.unlink()
                        
                        # Optimization: Clear all other pending records for this website since we just wiped everything
                        self.env.cr.execute(  # audit-ignore-sql: # Tested by [@ANCHOR: cf_process_queue_logic]  # fmt: skip
                            "DELETE FROM cloudflare_purge_queue "
                            "WHERE website_id = %s AND state = 'pending' AND id <= %s",
                            (first_website.id, covered_up_to_id),
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

            if first_website:
                if success:
                    self._resolve_purge_failure(first_website)
                else:
                    self._report_purge_failure(first_website, failure_reason, failure_severity)

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
