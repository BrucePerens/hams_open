# Story: Asynchronous Cache Purging

As a **Content Manager**,
I want my content updates to be reflected on the CDN edge immediately without slowing down my edit session,
so that users see the latest version of the site while I maintain high productivity.

## Scenario: Updating a Blog Post
1. I save a blog post in the Odoo backend.
2. The system calls `enqueue_tags` `[@ANCHOR: COMM_cf_enqueue_tags_api]`, `enqueue_urls` `[@ANCHOR: COMM_enqueue_urls_base_url]`, or `enqueue_everything` `[@ANCHOR: COMM_cf_enqueue_everything]`.
3. The purge requests are stored in the `cloudflare.purge.queue` model.
4. The background cron `[@ANCHOR: COMM_ir_cron_process_cf_purge_queue]` triggers.

5. The queue processor `[@ANCHOR: COMM_cf_process_queue_logic]` batches requests and communicates with the Cloudflare API to invalidate the cache.

**Status:** Verified by `[@ANCHOR: COMM_test_queue_batching_and_rate_limiting]`, `[@ANCHOR: COMM_test_purge_queue_base_url_sudo]`, `[@ANCHOR: COMM_test_purge_urls_api]`, `[@ANCHOR: COMM_test_purge_tags_api]`, and `[@ANCHOR: COMM_cf_enqueue_everything]`.

## Purging the Edge Cache When a Blog Post or Product Is Deleted

Archiving or editing a blog post or product goes through `write()`, which already queues a purge of its
URL. A real deletion used to leave the now-missing page cached at Cloudflare's edge indefinitely, because
nothing was left in Odoo to trigger a later purge. `blog.post.unlink()` and `product.template.unlink()` now
queue a purge of the record's `website_url` before calling the real delete (the URL can no longer be read
once the record is gone), the same order the website page and menu deletions use.
`[@ANCHOR: cloudflare:COMM_blog_post_unlink]` `[@ANCHOR: cloudflare:COMM_product_unlink]`
