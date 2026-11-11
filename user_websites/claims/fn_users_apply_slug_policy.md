---
anchor: user_websites:fn_users_apply_slug_policy
code_hash: sha256:00552c2f209b736def84826817bc8d2ca2c25a7c89aa44104b49f6dddb3d921d
hash_algorithm: v2
review_tier: 2
action_disposition: CLEAN
---

# Claim: `ResUsers._apply_slug_policy` (`user_websites/models/res_users.py`)

Read 2026-11-11 against the function's code and its callers.

1. IF the slug policy is not `callsign`, THEN THE function SHALL return `False` and leave `vals` untouched. HOLDS.
2. IF `vals` flags `is_service_account` or `record` is a service account, THEN THE function SHALL return `False` and leave `vals` untouched. HOLDS.
3. WHEN the identity is non-empty, AND `vals` carry `website_slug` or an identity field, AND the identity slugifies to a non-empty slug, THEN THE function SHALL call `assert_slug_free` (raising `ValidationError` if another member or group holds it) and set `vals["website_slug"]` to that slug in place. In every non-empty-identity case it SHALL return `False`. HOLDS.
4. WHEN the identity is empty AND `vals` carry `website_slug`, THEN THE function SHALL keep that slug and return `False`. HOLDS.
5. WHEN the identity is empty AND `vals` carry no `website_slug`, THEN THE function SHALL return `True` for `record is None` and otherwise `True` only if the record has no stored slug (it never clears one). HOLDS.
