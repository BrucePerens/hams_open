---
anchor: pager_duty:schedule_config_export
code_hash: sha256:8d048c4e130943c23ef53833636c4073f513ca396cbdc4356acc32be3af6976c
hash_algorithm: v2
review_tier: 2
action_disposition: CLEAN
---

# Claim: pager_check.py `PagerCheck._schedule_config_export` (`pager_duty/models/pager_check.py`)

Read 2026-11-11, whole function. Tier 2: Queues a rewrite of the daemon's config file after commit.

1. WHEN the transaction has already queued an export (the `pager_check_config_export` flag in the post-commit data) THE method SHALL do nothing, so any number of changes in one transaction queue one export.
2. OTHERWISE THE method SHALL set the flag and add one post-commit callback; it SHALL not write anything itself, so a rolled-back change never reaches the file.
3. THE callback SHALL open its own cursor as the pager service user and call `action_push_to_json`; a `danger` result is logged as a warning, and `OSError`, `UserError` and `AccessError` are logged and swallowed. Any other exception is not caught. HOLDS.
