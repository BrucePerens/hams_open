# Story: Automated WAF IP Banning

As a **Security Administrator**,
I want the system to automatically ban IP addresses that exhibit malicious behavior (like honeypot triggers),
so that the origin server is protected from further attacks.

## Scenario: Honeypot Trigger
1. A malicious actor attempts to access a hidden administrative URL.
2. The honeypot module detects this and calls `env['cloudflare.waf'].ban_ip(...)` `[@ANCHOR: COMM_cf_ban_ip_api]`.

3. The Cloudflare module executes the ban via `_execute_ban` `[@ANCHOR: COMM_cf_execute_ban]`.
4. A firewall rule is created at the Cloudflare edge to block the IP.
5. After the specified duration, the ban is automatically lifted by `_action_lift_ban` `[@ANCHOR: COMM_cf_action_lift_ban]`.

**Status:** Verified by `[@ANCHOR: COMM_test_cf_execute_ban]` and `[@ANCHOR: COMM_test_cf_action_lift_ban]`.

UI testing is verified by:
- IP Ban Tour: This tour verifies the IP ban functionality.
  `[@ANCHOR: COMM_cf_ip_ban_tour]`

- WAF Rule Tour: This tour verifies the WAF rule functionality.
  `[@ANCHOR: COMM_cf_waf_rule_tour]`

- Purge Wizard Tour: This tour verifies the purge wizard functionality.
  `[@ANCHOR: COMM_cf_purge_wizard_tour]`

- Zone Settings Tour: This tour verifies the zone settings functionality.
  `[@ANCHOR: COMM_cf_zone_settings_tour]`
Here is the URL purge API:
[@ANCHOR: COMM_cf_purge_urls_api]

Here is the Tag purge API:
[@ANCHOR: COMM_cf_purge_tags_api]

Here is the Backend views rendering:
[@ANCHOR: COMM_cf_backend_views_rendering]

