# High-Performance Atomic KV Storage [@ANCHOR: COMM_story_set_kv_procedure]

This story describes the implementation of high-performance atomic key-value storage using PostgreSQL procedures.

## Background
The `zero_sudo.security.utils` provides a lightweight KV storage abstraction for service accounts.
To optimize performance and ensure atomicity, we use a PostgreSQL procedure with an `ON CONFLICT` clause.

## Implementation Details
The `_set_kv` method delegates the operation to the `zero_sudo_set_kv` PostgreSQL procedure.
This eliminates the need for manual existence checks in Python and reduces the number of database round-trips to exactly one.

**Feature Anchor:** [@ANCHOR: COMM_set_kv_procedure]
KV procedures ensure atomic updates.


**Verified by:** [@ANCHOR: COMM_test_set_kv_procedure]

## Performance Benefits
* **Reduced Latency:** Exactly one database round-trip.
* **Atomicity:** Guaranteed by PostgreSQL's `INSERT ... ON CONFLICT`.
* **Zero-Sudo Compliance:** Operates via direct SQL, bypassing ORM overhead and `.sudo()` requirements.
