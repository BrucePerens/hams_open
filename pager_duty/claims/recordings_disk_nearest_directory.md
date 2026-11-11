---
anchor: pager_duty:recordings_disk_nearest_directory
code_hash: sha256:b2eed8c88c563f97330f4eed13bc99d0237f535f56672ecafc37b72b80cd139b
hash_algorithm: v2
review_tier: 3
action_disposition: CLEAN
---

# Claim: check_recordings_disk_usage.py `nearest_existing_directory` (`pager_duty/daemon/check_recordings_disk_usage.py`)

Read 2026-11-11, whole function. Tier 3: Pure path walk with `os.path.isdir`.

1. THE function SHALL return the absolute form of `path` when that is an existing directory.
2. OTHERWISE THE function SHALL return the closest existing ancestor directory of the absolute path, and at the filesystem root (whose parent is itself) it SHALL stop and return the root, so it always terminates. HOLDS. A path that exists as a regular file is treated as missing and its parent is returned.
