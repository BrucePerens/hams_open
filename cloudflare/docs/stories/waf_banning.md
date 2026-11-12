# Story: Automated WAF IP Banning

As a **Security Administrator**,
I want the system to automatically ban IP addresses that exhibit malicious behavior (like honeypot triggers),
so that the origin server is protected from further attacks.

## Scenario: Honeypot Trigger
1. A malicious actor attempts to access a hidden administrative URL.
2. The honeypot module detects this and calls `env['cloudflare.waf'].ban_ip(...)`.
3. The Cloudflare module executes the ban via `_execute_ban`.
4. A firewall rule is created at the Cloudflare edge to block the IP.
5. After the specified duration, the ban is automatically lifted by `_action_lift_ban`. (The [module README](../../README.md) states instead that each ban becomes a Cloudflare IP Access Rule with no expiry that stays until an administrator lifts it, and that there is no timed lift; step 4's "firewall rule" is that IP Access Rule.) The code agrees with the README: nothing calls the lift on a timer.

## Data: the `cloudflare.ip.ban` record

One row per banned address and website. `ip_address` (Char, required, not empty) is the address as given. `website_id` (Many2one, default the current website) names the zone the rule lives in; the pair `(ip_address, website_id)` is unique. `mode` is `block`, `challenge` or `managed_challenge` (the Cloudflare action applied, default `block`). `notes` (Char) is the trigger reason, default "Honeypot Triggered", or the failure text. `cf_rule_id` (Char, read-only) is the id of the Cloudflare access rule. `state` is `active`, `lifted` or `failed`. Rows are ordered newest first. Public and portal users have no access; the WAF service account and Settings administrators have full access.

## The ban entry points

### `ban_ip(ip_address, mode="block", duration=3600, notes="Honeypot Triggered", website_id=None)` [@ANCHOR: COMM_cf_ban_ip_api]

The public interface of `cloudflare.waf` that other modules (a honeypot, an abuse detector, an administrator tool) call to ban an address at the edge. It is `@api.private`, so it is not reachable over RPC. Arguments: `ip_address` string (the address to block; it is not validated here beyond being non-empty at the database); `mode` one of `block`, `challenge`, `managed_challenge`; `duration` an integer number of seconds that is accepted for callers' convenience but is not used: bans do not expire; `notes` a short reason stored on the record and sent to Cloudflare; `website_id` the website whose zone to use (default: the current one). The caller must be in the Cloudflare WAF group, a Settings administrator, or a service account; anyone else gets `AccessError`. It then runs `_execute_ban` as the WAF service account with mail tracking off, and returns that result: `True` when an active ban exists after the call, `False` when it could not be sent. Blocking call to Cloudflare; no retry. Test: `test_01_waf_ban_ip` in `tests/test_cloudflare_apis.py`.

### `_execute_ban(ip_address, mode="block", notes="Honeypot Triggered", website_id=None)` [@ANCHOR: COMM_cf_execute_ban]

The method on `cloudflare.ip.ban` that does the work. It resolves the website (default: current), reads that website's token and zone id, and looks for an existing row for the same address and website. An `active` row returns `True` at once and makes no second Cloudflare rule (a repeat honeypot hit once raised an IntegrityError from the unique constraint and returned a 500 to the visitor). Otherwise, with no credentials it writes a `failed` row ("Failed: Missing Cloudflare credentials.") and returns `False`; with credentials it calls `ban_ip` from the API layer, which POSTs an IP access rule to the zone. On success the row becomes `active` with the returned rule id in `cf_rule_id`; on failure it becomes `failed` with the error in `notes`, and a later call retries it by updating the same row. Returns a boolean; raises nothing for API problems. Must run as an account that may write the model (the WAF service account does). Blocking HTTP call (10 second timeout). It does not check who the caller is: that is `ban_ip`'s job. Tests: `test_01_cf_execute_ban` and the failure cases around it in `tests/test_waf_management.py`.

### `action_lift_ban()` [@ANCHOR: COMM_cf_action_lift_ban]

The button on the IP ban form ("Lift Ban"), declared `@public_rpc` so the client may call it. It acts on a recordset; for every record that is `active` and has a `cf_rule_id` it calls `_action_lift_ban_sync`, which sends a DELETE for that access rule to Cloudflare and, when Cloudflare agrees, sets the state to `lifted`. Records in other states are skipped silently. If Cloudflare refuses, `_action_lift_ban_sync` raises `UserError("Failed to lift ban via Cloudflare API: ...")` and the record stays `active`, so the administrator sees the problem and can retry. Nothing calls this on a timer; only an administrator (or code that calls it deliberately) lifts a ban. Returns `None`. Blocking network call per record. Test: `test_02_cf_action_lift_ban` in `tests/test_waf_management.py`.

## Tests of the ban flow

### `test_01_cf_execute_ban` [@ANCHOR: COMM_test_cf_execute_ban]

Patches the API-layer `ban_ip` to return `(True, "fake_rule_123")`, calls `_execute_ban("10.0.0.1", notes="Test Spam", website_id=...)` as the WAF service account, and asserts it returns `True`, that exactly one `cloudflare.ip.ban` row exists with state `active`, `cf_rule_id` equal to the fake id, and the right website. It proves the success path and the record it leaves behind.

### `test_02_cf_action_lift_ban` [@ANCHOR: COMM_test_cf_action_lift_ban]

Creates an active ban with a rule id, patches `unban_ip` first to fail (`(False, "Edge Offline")`), asserts `action_lift_ban()` raises `UserError` and the state is still `active`; then patches it to succeed and asserts the state becomes `lifted`. It proves that a Cloudflare refusal never marks a ban lifted, and that the sync helper `_action_lift_ban_sync` is reached through the button method.

## UI testing

UI testing is verified by the four browser tours in `tests/test_ui_tours.py` (class `TestCloudflareUITours`, which seeds one ban and one WAF rule first). Each tour runs as the administrator in a real browser and is a smoke test of a menu path and its form.

### IP Ban Tour [@ANCHOR: COMM_cf_ip_ban_tour]

`test_01_ip_ban_tour` starts the JS tour `cf_ip_ban_tour` at `/odoo?debug=1`. The tour opens the Cloudflare Edge app, the IP Bans list, waits for the seeded row (`192.168.9.9`), opens its form and checks that the "Lift Ban" button renders. This verifies the IP ban functionality: the list view, the form view and the button's presence. The tour file itself carries `COMM_test_tour_cf_ip_ban`.

### WAF Rule Tour [@ANCHOR: COMM_cf_waf_rule_tour]

`test_02_waf_rule_tour` starts `cf_waf_rule_tour`: it opens Cloudflare Edge, WAF Rules, waits for the action column and asserts that the seeded "Tour XML-RPC Rule" appears in the list. This verifies that WAF rule records render for an administrator. It does not push anything to Cloudflare.

### Purge Wizard Tour [@ANCHOR: COMM_cf_purge_wizard_tour]

`test_03_purge_wizard_tour` patches the credentials to a fake token and zone and `purge_everything` to return `True`, then runs `cf_purge_wizard_tour`: Cloudflare Edge, Purge Wizard, a modal opens, the Purge button (`action_purge`, default type "Purge Everything") is clicked, and the modal must close. The anchor sits on the wizard model `cloudflare.purge.wizard` in `models/purge_wizard.py`. No real Cloudflare call is made.

### Zone Settings Tour [@ANCHOR: COMM_cf_zone_settings_tour]

`test_04_zone_settings_tour` patches the credentials, `get_zone_settings` (returns an empty list) and `update_zone_setting` (returns success), then runs `cf_zone_settings_tour`: it opens the Zone Settings menu entry (using the "more" menu when the entry is hidden), waits for the modal, clicks "Apply Settings" (`action_apply_settings`) and waits for the modal to close. It verifies the zone settings wizard opens and applies with no real network call.

## The purge APIs

### URL purge API [@ANCHOR: COMM_cf_purge_urls_api]

`purge_urls(urls, token, zone_id)` in `utils/cloudflare_api.py` is the one function that sends URL purges to Cloudflare. It POSTs `{"files": [...]}` to `/zones/<zone_id>/purge_cache` with a `Bearer` token and a 10 second timeout, in chunks of 30 URLs. It returns `False` without a request when the token or zone id is missing, `True` without a request for an empty list, and otherwise `True` only if every chunk answered HTTP 200 (a network error or any other status makes it `False`, after logging). It does not retry a POST except on HTTP 429. Called by `process_queue` and the manual purge wizard. Test: `test_06_purge_urls` in `tests/test_cloudflare_apis.py`.

### Tag purge API [@ANCHOR: COMM_cf_purge_tags_api]

`purge_tags(tags, token, zone_id)` is the same call for Cache-Tag values: it POSTs `{"tags": [...]}` in chunks of 30, with the same credentials check, the same 10 second timeout and the same result rules (`False` for missing credentials or any failed chunk, `True` for an empty list). Callers are `process_queue` for queued tag rows and the purge wizard's "Purge Specific Cache-Tags" type. Test: `test_07_purge_tags` in `tests/test_cloudflare_apis.py`.

## Backend views

### Backend views rendering [@ANCHOR: COMM_cf_backend_views_rendering]

`test_05_backend_views_rendering` in `tests/test_ui_tours.py` asks the ORM for the compiled list or form view of nine backend models and asserts one expected field in each arch: `cloudflare.config.backup` (`create_date`), `cloudflare.ip.ban` (`ip_address`), `cloudflare.tunnel.wizard` (`command`), `cloudflare.waf.rule` (`action`), `cloudflare.dns.record` (`content`), `cloudflare.zone.settings` (`ssl_mode`), `cloudflare.rate.limit` (`mitigation_action`), `cloudflare.cache.rule` (`edge_cache_ttl`) and `cloudflare.zero.trust.policy` (`policy_action`). A view that no longer compiles against its model fails here, without opening a browser.

**Status:** Verified by `test_01_cf_execute_ban` and `test_02_cf_action_lift_ban` in `tests/test_waf_management.py`; the tours, the purge APIs and the view rendering test are covered in the entries above.
