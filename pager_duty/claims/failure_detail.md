---
anchor: pager_duty:failure_detail
code_hash: sha256:b2b6647c73f3f3c59b45980b9a053b7f19aa39757c197ac935d5eab0bb7887e6
hash_algorithm: v2
review_tier: 2
action_disposition: CLEAN
---

# Claim: generalized_monitor.py `_failure_detail` (`pager_duty/daemon/generalized_monitor.py`)

Read 2026-11-11, whole function. Tier 2: Read-only systemctl query for report text.

1. THE function SHALL run `<exe> show <unit> -p Result -p StateChangeTimestamp` with a 10-second timeout and return a string, never raising.
2. WHEN the command cannot run, times out or exits non-zero THE function SHALL return an empty string.
3. OTHERWISE it SHALL return `<Result> since <timestamp>` with the leading weekday dropped when the first word of the timestamp does not start with a digit, only the parts present (a missing timestamp gives just the Result, both missing give an empty string). HOLDS.
