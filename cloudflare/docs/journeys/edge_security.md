# Journey: Managing Edge Security

This journey covers the lifecycle of an IP ban, from detection to expiration. The full interface of each function is in [Automated WAF IP Banning](../stories/waf_banning.md).

## Phase 1: Detection
- Malicious activity is detected by a honeypot or security module.

### `env['cloudflare.waf'].ban_ip(ip)` is called [@ANCHOR: COMM_cf_ban_ip_api]

`ban_ip(ip_address, mode="block", duration=3600, notes=..., website_id=None)` is the single door through which other modules ask for a ban. The caller must be in the Cloudflare WAF group, a Settings administrator or a service account, otherwise `AccessError`. It elevates to the WAF service account and runs `_execute_ban`, returning `True` when a ban is active afterwards. `duration` is accepted but ignored: bans have no expiry.

## Phase 2: Enforcement

### `cloudflare.ip.ban` creates a record and triggers `_execute_ban` [@ANCHOR: COMM_cf_execute_ban]

`_execute_ban` finds or creates the record for this address and website (unique together), returns `True` at once for an already active ban, writes a `failed` row when the website has no token or zone id, and otherwise calls the Cloudflare access rules API. On success the record is `active` and keeps the Cloudflare rule id; on failure it is `failed` with the error text, and a later call retries it.

- An API call is sent to Cloudflare to block the IP, as an IP Access Rule on the website's zone, with the action chosen by `mode`.

### The UI reflects the banned state [@ANCHOR: COMM_test_tour_cf_ip_ban]

The browser tour `cf_ip_ban_tour` (`static/tests/tours/ip_ban_tour.js`, run by `test_01_ip_ban_tour`) opens the Cloudflare Edge app and the IP Bans list as an administrator, waits for a seeded active ban row, opens its form and checks that the "Lift Ban" button is rendered. It shows that an active ban is visible and can be lifted from the backend. The two IP ban views mark themselves as tested by it.

## Phase 3: Resolution
- The ban duration expires. (Per the [module README](../../README.md), a ban made through `ban_ip()` has no expiry and stays until an administrator lifts it; read this phase as the lifting step, whether by an administrator or by whatever calls `_action_lift_ban`.)

### The cleanup logic triggers `_action_lift_ban` [@ANCHOR: COMM_cf_action_lift_ban]

`action_lift_ban()` is the button method (the name in the code has no leading underscore). For each active record that has a Cloudflare rule id it sends a DELETE for that rule and, if Cloudflare agrees, sets the record to `lifted`; Cloudflare's refusal raises a `UserError` and the record stays `active`. Records in other states are skipped. Nothing calls it automatically.

- The IP is removed from Cloudflare's firewall (its IP Access Rule is deleted), and the row stays in the list as `lifted`, so a later ban of the same address updates this row instead of creating another.
