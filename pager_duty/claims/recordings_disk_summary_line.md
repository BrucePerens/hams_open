---
anchor: pager_duty:recordings_disk_summary_line
code_hash: sha256:b9dc03cd403e7f056ac5a7574a14dceaf2c2983ad4fd06b66b912184112e4320
hash_algorithm: v2
review_tier: 3
action_disposition: CLEAN
---

# Claim: check_recordings_disk_usage.py `summary_line` (`pager_duty/daemon/check_recordings_disk_usage.py`)

Read 2026-11-11, whole function. Tier 3: Pure formatting.

1. THE function SHALL return one line `recordings in <path>: <count> files, <total>, <rate>/day (up to the last 7 days)<partial>; filesystem <percent>% used, <tail>`, sizes through `human`, the percent with no decimals.
2. THE tail SHALL be `no growth seen` for `days_to_fail` None, `already past N%` for a value of 0 or less, `more than 10 years to N%` above 3650 days, otherwise `about D days to N%`, where N is `fail`.
3. WHEN the volume's `complete` flag is false THE function SHALL add ` (listing stopped early, partial)` after the rate; otherwise nothing. HOLDS.
