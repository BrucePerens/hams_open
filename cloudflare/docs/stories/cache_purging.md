# Story: Asynchronous Cache Purging

As a **Content Manager**,
I want my content updates to be reflected on the CDN edge immediately without slowing down my edit session,
so that users see the latest version of the site while I maintain high productivity.

## Scenario: Updating a Blog Post
1. I save a blog post in the Odoo backend.
2. The system calls `enqueue_tags` `[@ANCHOR: cf_enqueue_tags_api]`, `enqueue_urls` `[@ANCHOR: enqueue_urls_base_url]`, or `enqueue_everything` `[@ANCHOR: cf_enqueue_everything]`.
3. The purge requests are stored in the `cloudflare.purge.queue` model.
4. The background cron `[@ANCHOR: ir_cron_process_cf_purge_queue]` triggers.
5. The queue processor `[@ANCHOR: cf_process_queue_logic]` batches requests and communicates with the Cloudflare API to invalidate the cache.

**Status:** Verified by `[@ANCHOR: test_queue_batching_and_rate_limiting]`, `[@ANCHOR: test_purge_queue_base_url_sudo]`, `[@ANCHOR: test_purge_urls_api]`, `[@ANCHOR: test_purge_tags_api]`, and `[@ANCHOR: cf_enqueue_everything]`.
