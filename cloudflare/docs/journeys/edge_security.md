# Journey: Managing Edge Security

This journey covers the lifecycle of an IP ban, from detection to expiration.

## Phase 1: Detection
- Malicious activity is detected by a honeypot or security module.
- `env['cloudflare.waf'].ban_ip(ip)` is called `[@ANCHOR: COMM_cf_ban_ip_api]`.

## Phase 2: Enforcement
- `cloudflare.ip.ban` creates a record and triggers `_execute_ban` `[@ANCHOR: COMM_cf_execute_ban]`.
- An API call is sent to Cloudflare to block the IP.
- The UI reflects the banned state `[@ANCHOR: COMM_test_tour_cf_ip_ban]`.

## Phase 3: Resolution
- The ban duration expires.
- The automated cleanup logic triggers `_action_lift_ban` `[@ANCHOR: COMM_cf_action_lift_ban]`.
- The IP is removed from Cloudflare's firewall.
