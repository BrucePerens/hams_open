<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: Coherent Cache Signaling `[@ANCHOR: zero_sudo:COMM_story_cache_signaling]`

This story describes how the system ensures cache consistency across multiple Odoo workers.

## Background
In a clustered Odoo environment, if one worker updates a record, other workers might still have the old version in their local RAM cache.

## The Process
1. **Data Change**: A record is updated or a significant event occurs that requires cache invalidation.
2. **Notification**: The `_notify_cache_invalidation` function `[@ANCHOR: zero_sudo:COMM_coherent_cache_signal]` is called with the model name and the key (or keys) to invalidate.


3. **Postgres NOTIFY**: The function delegates to `distributed_redis_cache.redis_cache.notify_model_invalidation()`, which issues a `NOTIFY` command to the PostgreSQL database on the `distributed_cache_invalidation` channel, with a JSON payload (`{"model": ..., "dbname": ...}`). Bug-hunt fix, 2026-09-13: this function used to build its own payload (a plain colon-joined string, on a DIFFERENT channel, `"cache_invalidation"`) that the real listener below never actually consumed -- confirmed by reading `distributed_redis_cache/daemons/cache_manager.py`'s own `PG_CHANNEL` constant and its `json.loads(payload)` call. This is whole-model granularity only; the per-key/batch distinction this story previously described was never actually consumed downstream.
4. **Listener Action, corrected 2026-09-13**: `distributed_redis_cache/daemons/cache_manager.py` listens on the `distributed_cache_invalidation` channel and republishes the payload to Redis's `odoo_cache_invalidation_bus` pub/sub channel -- but no OTHER worker process actually subscribes to that channel (confirmed by grep: the only `pubsub()`/`subscribe()` call anywhere in either repo outside the daemon's own publish-side code is a test verifying the publish side, not a real consumer). Immediately invalidating this model's own Redis keys, and this ORIGINATING worker's own matching local-cache entries, is real and synchronous (see `invalidate_model_cache()`'s own claim) -- but the only thing that ever clears an OTHER worker's own local cache is the SEPARATE, unrelated poll in `distributed_redis_cache/models/ir_http.py`'s `IrHttp._authenticate`, which increments-and-compares a `global_cache_invalidation_counter` on every HTTP request and, on a change, clears that worker's ENTIRE local cache (every model, not scoped to just this one) -- see `redis_cache_interceptor.md`'s own claim for the full mechanism and its own `WorkerCron` gap. This correction mirrors that same claim's own 2026-09-09 correction of the identical "background listener thread"/"workers subscribe" misdescription found across 4 other docs -- this file was evidently missed by that earlier pass.

## Technical Detail
This uses the PostgreSQL `pg_notify` function to broadcast invalidation signals efficiently.

## Invalidate Model Cache
This feature `[@ANCHOR: zero_sudo:COMM_invalidate_model_cache]` allows invalidation of the model cache.
