# Story: Asynchronous Cache Purging

As a **Content Manager**,
I want my content updates to be reflected on the CDN (content delivery network) edge immediately without slowing down my edit session,
so that users see the latest version of the site while I maintain high productivity.

## Scenario: Updating a Blog Post
1. I save a blog post in the Odoo backend.
2. The system calls `enqueue_tags`, `enqueue_urls`, or `enqueue_everything` on `cloudflare.purge.queue`.
3. The purge requests are stored as rows of the `cloudflare.purge.queue` model, and the save returns at once.
4. The background cron `ir_cron_process_cf_purge_queue` triggers, every minute.
5. The queue processor `process_queue` batches requests per website and communicates with the Cloudflare API to invalidate the cache.

The entries below describe each step. Every entry names the code it documents and the test that cites it.

## Data: the `cloudflare.purge.queue` row

One row is one pending or failed purge. `purge_type` is `url`, `tag` or `everything`; `target_item` holds the
absolute URL or the cache tag (empty for `everything`); `website_id` names the website whose zone and credentials
are used (cascade delete); `state` is `pending` (default, indexed) or `failed`. The row never holds a token: the
processor fetches credentials from the website each time. Only the purge service account, the WAF service account
and Settings administrators have an access row.

## Entry points

### `enqueue_urls(urls, website_id=None)` [@ANCHOR: COMM_enqueue_urls_base_url]

Queues a purge of page URLs for one website and returns nothing. It is a private model method (`@api.private`, `@api.model`), so only server code calls it, usually through the purge mixin. `urls` is a list of strings, either absolute (`https://hams.com/blog/x`, kept as is) or site-relative (starting with `/`, which is joined to the website's `domain`, or to the `web.base.url` system parameter when the website has no domain). `website_id` is an integer; when empty it is the website of the current request, or the default website in a cron or shell (`get_current_website_id`). It delegates to `enqueue_urls_batch`, which removes empty values, de-duplicates against rows that are already pending for the same website and URL, and inserts only the new ones, so saving a page ten times queues one purge. It does not call Cloudflare and takes no lock beyond the transaction. Tests: `test_03_purge_queue_website_acl` and the batch tests in `tests/test_enqueue_urls_batch.py`.

### `enqueue_tags(tags, website_id=None)` [@ANCHOR: COMM_cf_enqueue_tags_api]

Queues a purge of Cloudflare `Cache-Tag` values for one website. `tags` is a list of strings (empty values are ignored); `website_id` is resolved as in `enqueue_urls`. One `tag` row is created per tag, in a single `create`. Unlike URLs, tags are not de-duplicated at insert time, so queueing the same tag twice sends it twice. The tags the edge module attaches are `odoo-static-assets` and `odoo-website-<id>`, which lets a caller purge a whole website or every compiled asset bundle at once. Private (`@api.private`), returns nothing, never talks to Cloudflare, and raises nothing for empty input. The tags are sent later by `process_queue` in batches of 30. Tests: `test_04_purge_queue_tags_processing` in `tests/test_purge_queue.py` queues three tags, runs the processor with the HTTP session mocked, and asserts that one request went out and no tag row remains.

### `enqueue_everything(website_ids=None)` [@ANCHOR: COMM_cf_enqueue_everything]

Queues a full "purge everything" request for each given website, for example after a menu changed or an administrator needs a clean edge. `website_ids` may be a list, tuple or set of ids, a single id, or empty (then the current website is used). Falsy ids are skipped. One `everything` row is created per website. Private, returns nothing, and does not call Cloudflare. When the processor handles an `everything` row it also deletes every other pending row of that website that existed before the API call, because the wipe already covers them; rows queued while the call was in flight are kept, since they may describe newer content. Tests: `tests/test_purge_everything.py`, which cites this anchor, and `COMM_cf_process_queue_logic` for the processor side.

### `ir_cron_process_cf_purge_queue` (scheduled action) [@ANCHOR: COMM_ir_cron_process_cf_purge_queue]

The `ir.cron` record "Cloudflare: Process Cache Purge Queue" in `data/cron.xml`. It runs `model.process_queue()` on `cloudflare.purge.queue` every 1 minute, active by default, as the purge service user (`cloudflare.user_cloudflare_purge`), and is `noupdate`, so an administrator's changed interval survives upgrades. When one run is not enough to empty the queue, `process_queue` asks Odoo to trigger this cron again at once instead of waiting a minute. Concurrency: Odoo runs one instance of a given cron at a time, so two processors never race on the same rows. Test: `test_01_bdd_queue_batching_and_rate_limiting` in `tests/test_purge_queue.py`, which cites the cron and the processor together.

### `process_queue()` [@ANCHOR: COMM_cf_process_queue_logic]

The queue processor, called by the cron. It switches to the purge service account, first calls `requeue_failed()` (rows that failed on an API error at least an hour ago go back to `pending` when their website has credentials), then loops at most 10 times. Each loop takes up to 30 pending rows ordered by website, keeps only the first website's rows (credentials are never mixed between zones), and reads that website's token and zone id. With no token or zone id the rows are logged and dropped, and a medium pager incident is raised. Otherwise it sends `purge_everything` first if present, then `purge_urls`, then `purge_tags`; rows that succeeded are deleted, rows that failed are marked `failed` and a high pager incident is raised, and a fully successful batch auto-resolves the website's incident. It commits after every batch and sleeps 0.1 second to release database locks and respect rate limits. If all 10 batches were used it triggers its own cron. Private; returns nothing. Not safe to call from two workers at once on the same rows; the cron guarantees one.

## Deleting content

### `BlogPost.unlink()` [@ANCHOR: cloudflare:COMM_blog_post_unlink]

Archiving or editing a blog post goes through `write()`, which already queues a purge of its URL. A real deletion used to leave the now-missing page cached at Cloudflare's edge indefinitely, because nothing was left in Odoo to trigger a later purge. `blog.post.unlink()` now calls `_enqueue_cloudflare_purge("website_url")` before `super().unlink()`, the same order the website page and menu deletions use; after the record is gone its `website_url` can no longer be read. Arguments are the recordset it is called on; it returns whatever the real delete returns and raises whatever it raises. The purge is only queued (the queue row commits or rolls back with the delete), and the cron sends it. Test: `test_blog_post_unlink_enqueues_a_purge` in `tests/test_bridge_and_misc.py`, which empties the queue, deletes a post and asserts a row for its URL was queued.

### `ProductTemplate.unlink()` [@ANCHOR: cloudflare:COMM_product_unlink]

The product equivalent of the blog entry above. `product.template.unlink()` queues a purge of each product's `website_url` before it calls the real delete, so a deleted product's page does not stay in Cloudflare's cache. It uses the same `cloudflare.purge.mixin` helper (`_enqueue_cloudflare_purge`), which fans a record without a website out to every website of the install, because such content really is served on every site. A product's `write()` queues the URL both before and after the change (so an old and a new URL are both purged). Returns the result of `super().unlink()`; no new error cases. Test: `test_bridge_and_misc.py`, `test_product_template_unlink_enqueues_a_purge`, which empties the queue, deletes a product and asserts a row for its URL was queued.

## Tests

### `test_01_bdd_queue_batching_and_rate_limiting` [@ANCHOR: COMM_test_queue_batching_and_rate_limiting]

Queues 310 URL rows for one website with fake credentials, mocks the HTTP session and `time.sleep`, runs `process_queue()` once, and asserts the batching and rate-limit rules: exactly 10 API requests (10 batches of 30 URLs), exactly 10 sleeps, 10 rows left over for the next run, and that the cron is re-triggered once because the batch limit was reached. It also marks the cron record in `data/cron.xml` as tested. It proves the cron's bounded work per run and the follow-up trigger.

### `test_03_purge_queue_website_acl` [@ANCHOR: COMM_test_purge_queue_base_url_sudo]

Creates a pending queue row as the purge service account, then reads `website_id.domain` as that account and asserts no `AccessError` is raised. It proves the service account may read the one website field that `enqueue_urls` needs to build absolute URLs, without any `sudo()` (forbidden on this platform). If a future access-rule change removed that read, this test fails before URL purges silently stop being queued.

### `test_06_purge_urls` [@ANCHOR: COMM_test_purge_urls_api]

In `tests/test_cloudflare_apis.py`. Exercises `purge_urls(urls, token, zone_id)` with the HTTP session mocked: missing token or zone returns `False` and sends nothing; an empty list returns `True` with no request; two URLs go out in one POST whose JSON is `{"files": [...]}` with a `Bearer` header; 40 URLs go out as two requests of 30 and 10; a network error returns `False` without raising. This documents the 30-per-request chunking that the queue processor relies on.

### `test_07_purge_tags` [@ANCHOR: COMM_test_purge_tags_api]

The sibling of the URL test, for `purge_tags(tags, token, zone_id)`: missing credentials return `False`, an empty list returns `True` with no request, a success sends `{"tags": [...]}` with the bearer token in chunks of 30, and a request failure returns `False`. It also covers the shared request helper (`_make_request`) and error handler, so the error log path stays exercised. Used by the queue test for tag rows as the transport check.

**Status:** Verified by the four tests above and by `COMM_cf_enqueue_everything`'s own test, `tests/test_purge_everything.py`.
