# Story: Deterministic Hashing `[@ANCHOR: story_deterministic_hash]`

This story describes the generation of stable, integer-based hashes for database locking.

## Background
PostgreSQL advisory locks (`pg_advisory_xact_lock`) require a 32-bit or 64-bit integer as a key. Often, we want to lock based on a string (like a resource name).

## The Process
1. **Input String**: A developer has a string that identifies a resource to be locked.
2. **Hash Generation**: The `_get_deterministic_hash` function `[@ANCHOR: deterministic_hash]` is called with the string.
3. **SHA-256 Conversion**: The function computes a SHA-256 hash of the string, takes a portion of it, and converts it to a 32-bit integer.
4. **Result**: The resulting integer is deterministic (the same string always produces the same integer) and can be used directly for advisory locks.

## Use Case
This is crucial for preventing race conditions in background tasks that must operate on a specific resource exclusively.
