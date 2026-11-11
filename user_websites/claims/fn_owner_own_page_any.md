---
anchor: user_websites:fn_owner_own_page_any
code_hash: sha256:24761763a7e327283ea93df174dac8800e14499275b09afa887785ab985641c3
hash_algorithm: v2
review_tier: 2
action_disposition: CLEAN
---

# Claim: `UserWebsitesOwnerController._own_page_any` (`user_websites/controllers/owner_site.py`)

Read 2026-11-11 against the function's code and its callers.

1. WHEN `page_id` is not an existing website.page, even an archived one, THE function SHALL raise `request.not_found()`. HOLDS (`.exists()` on an `active_test=False` browse).
2. IF `profile_user` is set and the page's `owner_user_id` is not it, OR `profile_group` is set and the page's `user_websites_group_id` is not it, THEN THE function SHALL raise `request.not_found()`. HOLDS.
3. OTHERWISE THE function SHALL return the page record, archived or not, without checking that its URL is under the owner's slug (unlike `_own_page`). HOLDS.
