---
anchor: pager_duty:freshness_redis
code_hash: sha256:fc28625284f8ffef4d35f035528f1afba655e22ce6e9d8bcd6d5984d52e8abed
hash_algorithm: v2
review_tier: 2
action_disposition: CLEAN
---

# Claim: generalized_monitor.py `_freshness_redis` (`pager_duty/daemon/generalized_monitor.py`)

Read 2026-11-11, whole function. Tier 2: Builds a client; no connection is made until first use.

1. THE function SHALL return a new `redis.Redis` for host `REDIS_HOST` (default `redis` when unset or empty) and port `REDIS_PORT` (default 6379).
2. THE client SHALL use the module's `REDIS_USERNAME` and `REDIS_PASS` (read from the environment at import time, not at the call) and 5-second socket and connect timeouts.
3. A non-numeric `REDIS_PORT` raises `ValueError`. HOLDS.
