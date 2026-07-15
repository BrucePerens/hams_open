<!-- Copyright © Bruce Perens K6BP. SPDX-License-Identifier: AGPL-3.0-or-later -->

# Journey: Request Caching Lifecycle

This journey describes the path of a data retrieval request when using the distributed cache.

1. **Function Call**: An Odoo business logic function decorated with `@distributed_cache()` ([@ANCHOR: COMM_distributed_cache_decorator]) is invoked.

2. **Key Generation**: The decorator serializes the function arguments and generates a unique HMAC-SHA256 hash ([@ANCHOR: COMM_distributed_cache_key_generation]).

3. **Redis Lookup**: The system attempts to fetch the value from the Redis connection pool ([@ANCHOR: COMM_redis_connection_pool]).
4. **Cache Hit**:
   - If the key exists in Redis, the JSON-serialized data is retrieved and returned immediately.
5. **Cache Miss**:
   - If the key does not exist, the original function is executed to fetch data from the database.
   - The result is stored in Redis with a 24-hour TTL for future use.
6. **Fallback**:
   - If Redis is unreachable, the system checks the local memory fallback.
   - If not in memory, it fetches from the DB and stores the result locally.
