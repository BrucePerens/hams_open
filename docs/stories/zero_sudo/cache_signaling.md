# Story: Coherent Cache Signaling `[@ANCHOR: story_cache_signaling]`

This story describes how the system ensures cache consistency across multiple Odoo workers.

## Background
In a clustered Odoo environment, if one worker updates a record, other workers might still have the old version in their local RAM cache.

## The Process
1. **Data Change**: A record is updated or a significant event occurs that requires cache invalidation.
2. **Notification**: The `_notify_cache_invalidation` function `[@ANCHOR: coherent_cache_signal]` is called with the model name and the key (or keys) to invalidate.
3. **Postgres NOTIFY**: The function issues a `NOTIFY` command to the PostgreSQL database.
4. **Listener Action**: Other workers (or a dedicated cache manager daemon) listening on the `cache_invalidation` channel receive the payload and clear the corresponding local caches.

## Technical Detail
This uses the PostgreSQL `pg_notify` function to broadcast invalidation signals efficiently.
