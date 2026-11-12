# Journey: High-Performance Content Invalidation

This journey describes how cache invalidation is handled efficiently in the background. The full interface of each function is in [Asynchronous Cache Purging](../stories/cache_purging.md); the entries here say what each one does in this journey.

## Phase 1: Invalidation Request
- A record update triggers a cache purge requirement: saving or deleting a page, blog post, product or menu calls the purge mixin, which queues rows and returns at once.

### URLs are enqueued via `enqueue_urls` [@ANCHOR: COMM_enqueue_urls_base_url]

`enqueue_urls(urls, website_id=None)` takes site-relative paths (joined to the website's domain, or to the base URL parameter when it has none) or absolute URLs, and inserts one pending `url` row per target that is not already pending for that website. The caller never waits for Cloudflare. It is private and returns nothing. Saving the same page repeatedly therefore queues one purge, not many.

### Tags are enqueued via `enqueue_tags` [@ANCHOR: COMM_cf_enqueue_tags_api]

`enqueue_tags(tags, website_id=None)` inserts one pending `tag` row per non-empty Cache-Tag value, for example `odoo-website-3` or `odoo-static-assets`, which the edge module puts on cacheable responses. Use it when a whole group of pages must be invalidated without listing their URLs. Private, returns nothing, does not contact Cloudflare.

## Phase 2: Queue Management
- Requests are persisted in `cloudflare.purge.queue`, with the website they belong to and a state of `pending` or `failed`.
- The system continues processing user requests without waiting for the CDN.

## Phase 3: Background Processing

### The Odoo cron runs periodically [@ANCHOR: COMM_ir_cron_process_cf_purge_queue]

The scheduled action "Cloudflare: Process Cache Purge Queue" runs `process_queue()` every minute as the purge service user. It is `noupdate`, so a changed interval survives upgrades. When a run hits its batch limit with work left, the processor triggers this cron again immediately, so a large backlog drains in successive runs instead of one slow one.

### `process_queue` batches records by website [@ANCHOR: COMM_cf_process_queue_logic]

`process_queue()` takes up to 30 pending rows per batch and at most 10 batches per run, always for one website at a time so credentials are never mixed between zones. Rows for a website with no API token or zone id are dropped and a pager incident is raised. Successful rows are deleted; failed rows are marked `failed`, retried once an hour, and raise a pager incident.

- Purge commands are sent to Cloudflare in batches (per the [module README](../../README.md): the cron runs every minute, up to 10 batches of 30 entries per run; entries for a website with no API token or Zone ID, or whose API call fails, are marked `failed` and not retried). The code differs in two ways: rows without credentials are deleted rather than marked, and failed rows are retried once, after an hour.

### Status

**Status:** Verified by `test_01_bdd_queue_batching_and_rate_limiting` in `tests/test_purge_queue.py`, which queues 310 URLs and asserts exactly 10 requests, 10 sleeps, 10 rows left and one cron re-trigger.

### The batching test [@ANCHOR: COMM_test_queue_batching_and_rate_limiting]

The test patches the HTTP session and `time.sleep`, queues 310 URL rows for one website, runs `process_queue()` once and checks that 10 requests went out (30 URLs each), that it slept after every batch to release database locks, that 10 rows remain, and that the cron was triggered again. It is the proof of the bounded work per run and of the rate-limit pause.
