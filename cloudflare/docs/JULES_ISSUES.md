# Jules Issues - Cloudflare Module

## Odoo 19 Compliance & Stability
- Repaired `cf_ip_ban_tour` and `cf_waf_rule_tour` to follow Odoo 19 mandates.
- Removed banned `:contains` pseudo-selectors from all tours.
- Added explicit wait steps (`.o_list_renderer`, `.o_form_sheet`) to ensure tours wait for asynchronous UI rendering before interaction.
- Enhanced Python test assertions with `[!] DIAGNOSTIC FOR AI:` messages to aid autonomous troubleshooting.

## Security & Isolation
- Multi-tenancy isolation is enforced at the model level for all Cloudflare-related entities.
- Dedicated Zero-Sudo service accounts are used for background edge operations (Purge, WAF, Tunnels), following the principle of least privilege.
- Verified that credentials and settings are website-specific and never exposed to the frontend.

## Record Rules Note
- Explicit record rules using `user.website_id` were evaluated but deferred due to inconsistency in attribute availability on `res.users` in this environment's Odoo 19 version. Stability is maintained through backend filtering and UI scoping.

## Miscellaneous
- Fixed a selector issue in `ip_ban_tour.js` where it was incorrectly targeting `td[name="ip_address"]` instead of `td` or `td[data-name="ip_address"]`.
- Removed an unrelated `erl_crash.dump` file from the repository root.
