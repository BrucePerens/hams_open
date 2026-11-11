---
anchor: cloudflare:COMM_settings_compute_trusted_ip_display
code_hash: sha256:8dff12b045128d3ac2b1e6391783c2ceeb8b9141f2dc5d5ffcb30b52c5601593
hash_algorithm: v2
review_tier: 1
---

# Claim: `_compute_cloudflare_trusted_ip_ranges_display`

`_compute_cloudflare_trusted_ip_ranges_display()` fills the read-only `cloudflare_trusted_ip_ranges_auto` and `cloudflare_trusted_ip_ranges_last_refreshed` fields from the `cloudflare.trusted_ip_ranges_auto` and `..._last_refreshed` parameters, read as the `user_cloudflare_trusted_ip` service user, showing a "not yet refreshed" text and "Never" when they are empty.

Read against the function body on 2026-11-11: HOLDS. Not verified here: callers outside the cloudflare module.
