---
anchor: pager_duty:recordings_disk_human
code_hash: sha256:3cdada5ee9dba8f58d4b9b6803f9abcd7346110e3e6e0509b9632691363dec06
hash_algorithm: v2
review_tier: 3
action_disposition: CLEAN
---

# Claim: check_recordings_disk_usage.py `human` (`pager_duty/daemon/check_recordings_disk_usage.py`)

Read 2026-11-11, whole function. Tier 3: Pure formatting.

1. THE function SHALL divide by 1000 (decimal units) through B, KB, MB, GB, TB and return the first unit in which the value is under 1000, or TB for anything larger.
2. WHEN the unit is B the result SHALL be an integer plus ` B` (`999 B`); otherwise one decimal plus the unit (`1.5 KB`, `2.0 GB`).
3. A value just under a unit boundary that rounds up prints as `1000.0 KB` (for example 999950 bytes); this is the only oddity, and the function never raises for a number. HOLDS.
