---
anchor: hams_helpdesk:portal_my_tickets
code_hash: sha256:663edddf6c2c43c10599ce7f53733df8f9dabd948b21abc519c216922cef68c6
hash_algorithm: v2
review_tier: 1
action_disposition: CLEAN
---

# Claim: `portal.HelpdeskPortal.portal_my_tickets`

THE route SHALL require a logged-in user (`auth="user"`) and render `hams_helpdesk.portal_my_tickets` with the tickets of `request.env.user.partner_id` only, searched as the helpdesk service user in the request website's company, newest `create_date` first, 20 per page. WHEN the request has a website, THE domain SHALL include only tickets whose `website_id` is that website or unset. The pager's `total` is `search_count` of the same domain. HOLDS. Read 2026-11-11 against the code.
