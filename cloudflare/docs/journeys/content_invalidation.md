# Journey: High-Performance Content Invalidation

This journey describes how cache invalidation is handled efficiently in the background.

## Phase 1: Invalidation Request
- A record update triggers a cache purge requirement.
- URLs are enqueued via `enqueue_urls` `[@ANCHOR: COMM_enqueue_urls_base_url]`.

- Tags are enqueued via `enqueue_tags` `[@ANCHOR: COMM_cf_enqueue_tags_api]`.

## Phase 2: Queue Management
- Requests are persisted in `cloudflare.purge.queue`.
- The system continues processing user requests without waiting for the CDN.

## Phase 3: Background Processing
- The Odoo Cron `[@ANCHOR: COMM_ir_cron_process_cf_purge_queue]` executes periodically.

- `process_queue` `[@ANCHOR: COMM_cf_process_queue_logic]` batches records by website/credentials.
- Purge commands are sent to Cloudflare in batches (per the [module README](../../README.md): the cron runs every minute, up to 10 batches of 30 entries per run; entries for a website with no API token or Zone ID, or whose API call fails, are marked `failed` and not retried).

**Status:** Verified by `[@ANCHOR: COMM_test_queue_batching_and_rate_limiting]`.
