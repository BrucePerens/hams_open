# Cloudflare Edge Orchestration (`cloudflare`)

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).*

This module acts as the command center for your Cloudflare CDN (Content Delivery Network, Cloudflare's worldwide edge cache) and Web Application Firewall (WAF). It automates edge caching, security, and IP bans across multiple websites, eliminating the need to manually manage these settings in the Cloudflare dashboard.

## 🌟 What It Does

* **Multi-Website Support:** Seamlessly manage multiple domains/zones from a single Odoo instance. Credentials and settings are isolated per website.
* **DNS & Routing:** Keep a list of DNS records in Odoo (`cloudflare.dns.record`; types A, AAAA, CNAME, TXT, NS). Editing a row changes nothing at Cloudflare until an administrator pushes it: a push shows a plan first, and applies exactly that plan. It creates and updates only the records Odoo has a row for, and deletes nothing except a record whose row is marked **Retire** (see Technical Documentation). Supports multi-tenant Custom Hostname SSL provisioning, allowing tenants to bring their own domains (`CloudflareRoutingDomain`, this module's extension of `edge.routing.domain`): when a domain mapping is created whose name equals a website's domain and that website has an API token and Zone ID, the module asks Cloudflare to create a custom hostname with a domain-validated certificate; with no matching website, provisioning is skipped. The SSL status stored in Odoo is set from Cloudflare's reply at creation and is refreshed only by calling `action_sync_ssl_status()` (administrators and service accounts only); no cron or button calls it. If Cloudflare refuses to delete a hostname, the local mapping is still deleted and a retry record appears under **Pending Hostname Deletes**.
* **Automated Static Caching & Purging:** It tells Cloudflare to cache Odoo's asset bundles (the combined CSS and JS served under `/web/assets`) for a full year, tagged `odoo-static-assets`. Images and downloads served by `/web/image` and `/web/content` are never edge-cached, because they can be private attachments; per-module `/static/` files do not pass through this hook and keep Odoo's own `Cache-Control`. A static-file change detector (`cloudflare.config.manager._trigger_edge_purge_static_assets`) queues an `odoo-static-assets` purge for every website with credentials when a static file is newer than the last recorded change, but no code path currently calls it at server start, so it does not run on boot by itself. The tag can be purged by hand with the Manual Cache Purge wizard.
* **Intelligent Content Invalidation:** Automatically enqueues purges for specific URLs when website pages, blog posts, or products are edited or deleted. A website menu change enqueues a purge of that website's whole cache, since menus appear on every page.
* **Advanced Caching & Rate Limiting:** Record Cloudflare Cache Rules and Rate Limits in Odoo. These are stored in Odoo only: the module has no Cloudflare API call that deploys them, so they have no effect on the edge until configured in Cloudflare itself.
* **WAF Management:** Build and deploy Cloudflare custom firewall rules from the Odoo backend. **Pull** replaces this website's rules in Odoo with the zone's current custom firewall ruleset; **Push** replaces the zone's whole custom firewall ruleset with the rules in Odoo, so a rule that exists only in Cloudflare is removed. No backup is taken on Push (see Configuration Backups).
* **Honeypot IP Banning:** Ban malicious IPs at the network edge. This module contains no honeypot trap itself; it provides `cloudflare.waf.ban_ip()` for honeypot features in other modules to call. Each ban becomes a Cloudflare IP Access Rule on the website's zone and has no expiry: it stays until an administrator lifts it.
* **Zero Trust & Access:** Provision and manage `cloudflared` tunnels directly from Odoo to secure internal resources. Zero Trust Policies can be recorded in Odoo, but like Cache Rules they are stored in Odoo only and are not pushed to Cloudflare.
* **Turnstile Integration:** Backend validator for Cloudflare's invisible Turnstile CAPTCHA. Without a Turnstile Secret configured for the website, every token is rejected.
* **Analytics Dashboard:** Client action dashboard intended for traffic, security, and performance analytics. It is currently a placeholder: it shows fixed sample numbers, not data fetched from Cloudflare.
* **Configuration Backups:** "Pre-Odoo" backups: when the module is installed, each website whose zone already has custom firewall rules gets a read-only snapshot of that ruleset's raw JSON, taken before those rules are imported into Odoo (the zone's state before Odoo took the rules over). Zones without existing rules instead receive the module's default rules. Backups are taken only at install, not before later pushes, and there is no restore action.
* **Zone Settings Control:** Read and adjust security level, development mode, and browser cache TTL per website, applied to Cloudflare immediately.

## 🚀 Odoo 19 & Zero-Trust Ready

This module is fully optimized for Odoo 19 and adheres to a strict Zero-Sudo architecture: it never uses Odoo's `.sudo()`; work that needs more rights than the current user has runs as a dedicated service-account user (`with_user()`), each holding only the rights its job needs. All Cloudflare edge operations are delegated to specific service accounts, ensuring that even in a multi-tenant environment, credentials and operations remain isolated and secure.

## 🛠️ How to Set It Up

1. Ensure the `cloudflare` module is in your Odoo `addons` directory.
2. Configure credentials per website in **Settings > Cloudflare** (the **Cloudflare Edge Orchestration** section):
   * `CF API Token` (Requires `Zone.Cache Purge`, `Zone.Firewall Services`, and `Account.Cloudflare Tunnel` permissions)
   * `CF Zone ID`
   * `CF Account ID` (Required for Zero Trust Tunnels)
   * `Turnstile Secret` (Optional; required for Turnstile verification)

---

# Technical Documentation

## 1. Overview
Control plane for the CDN edge. Manages Cache-Tags (labels sent in a response's `Cache-Tag` header so Cloudflare can later purge every cached object carrying that label; this module sets `odoo-static-assets` and `odoo-website-<id>`), WAF bans, and Turnstile CAPTCHA verification to offload processing to Cloudflare's edge.

## 2. API Interfaces
* **DNS & Hostnames:** `cloudflare.dns.record` rows (A, AAAA, CNAME, TXT, NS) are pushed to Cloudflare by `dns_push_plan()` and `dns_push_apply(hash)` (list view, Action, "Push DNS records to Cloudflare"; administrators only). Odoo is the source of truth. `CloudflareRoutingDomain` provisions Custom Hostnames separately; never add one for a name that is already a zone of the account.
  * **Plan first.** The plan is computed from reads only and shows, per row: `create`, `update`, `delete`, `adopt`, `conflict`, `problem`, `drift`, `unchanged`, with the reason. Apply takes the plan's hash and refuses if Cloudflare or the rows changed since. A row that cannot be planned (no token, no zone, Cloudflare unreachable) is reported and does not hold back the others; an unreachable Cloudflare plans nothing, never "create everything".
  * **What it never does.** It never overwrites a record it did not create: a record that already exists with the same content (and proxied flag) is *adopted*, which stores its id in the row and writes nothing at Cloudflare; one with other content is a `conflict`. Pasting that record's id into the row is the deliberate way to take it over. It never touches a record Odoo has no row for. Deleting or archiving a row removes nothing at Cloudflare; the only delete is the **Retire** flag on a row linked to its record by id. An update keeps the record's TTL (a new record gets automatic TTL).
  * **Rows.** `name` is the full lower-case name without a trailing dot; the zone is found from it (the longest suffix that is a zone the token sees). Credentials: the row's website, else the first website with an API token. `manage` off means observe only (rows that existed before 1.7 were switched to it by the migration). NS and TXT rows, and any name an NS row points at (glue), are never proxied.
  * **DNSSEC.** `hams.com` is DNSSEC-signed; a delegated zone (`callbook.hams.com`, `u.hams.com`) that is not signed needs no DS record, and Odoo has no DS type: an NS delegation with no DS is an ordinary insecure delegation.
  * **Extension.** `cloudflare.dns.record._dns_problems(entries)` returns strings that refuse the whole push, as `cloudflare.tunnel._ingress_problems` does for tunnels.

* **Traffic Control:** Management of `cloudflare.rate.limit` and `cloudflare.cache.rule` (local records only, not synchronized to Cloudflare).

* **Zero Trust Policies:** Management of `cloudflare.zero.trust.policy` models (local records only, not synchronized to Cloudflare).

* **Configuration Backups:** Management of `cloudflare.config.backup` for pre-Odoo state snapshots (created at install by `initialize_cloudflare_state()`).

* **WAF IP Banning:** `env['cloudflare.waf'].ban_ip(...)` creates a Cloudflare IP Access Rule (block or challenge) for the address on the website's zone and records it in `cloudflare.ip.ban` `[@ANCHOR: cf_execute_ban]`. Supports multiple websites. Callers must be in the Cloudflare WAF group, be Settings administrators, or be service accounts. Repeat calls for an already-active ban do nothing; a ban that could not be sent (no credentials, API error) is recorded with state `failed` and retried on the next call.

* **WAF Management:** Pull `[@ANCHOR: cf_action_pull_waf_rules]` and push `[@ANCHOR: cf_action_push_waf_rules]` firewall rules.

* **Cache Purging:** `env['cloudflare.purge.queue'].enqueue_urls(...)` and `enqueue_tags(...)`. Processes an asynchronous queue grouped by website to prevent credential mixing `[@ANCHOR: cf_process_queue_logic]`. The "Process Cache Purge Queue" cron runs it every minute as the purge service account, up to 10 batches of 30 entries per run. A successful purge deletes its entries; an "everything" purge also drops that website's other pending entries. Entries for a website with no API token or Zone ID, or whose API call fails, are marked `failed` and are not retried.

* **Turnstile API:** `env['cloudflare.turnstile'].verify_token(...)` evaluates tokens against the Cloudflare API `[@ANCHOR: cf_turnstile_verify]`.

* **Edge Context:** `env['cloudflare.utils'].get_request_context()` extracts geographic and threat data from trusted headers `[@ANCHOR: cf_get_request_context]`. "Trusted" means the `CF-*` headers are used only when the connecting peer is loopback (the Tunnel case) or inside the trusted Cloudflare IP range allow-list (empty unless an administrator turns on **Trust Cloudflare's Published IP Ranges** in Settings, for a deployment without a Tunnel); from any other peer every geographic and threat field is returned as `None`, so a direct request to the origin cannot forge them.

* **Tunnel Management:** Wizard generates installation commands `[@ANCHOR: cf_tunnel_setup]`. Sync and delete tunnels across accounts `[@ANCHOR: cf_sync_tunnels]`, `[@ANCHOR: cf_delete_tunnel]`.

* **Multi-website tunnels:** one server fronting several websites is the ordinary case here. The "Ensure Tunnel Daemon Running" cron keeps **every** tunnel that has credentials up `[@ANCHOR: ensure_tunnel_running]`, one `cloudflared` daemon per tunnel, each tracked under its own Cloudflare tunnel id: a running tunnel is never started twice, a dead one is restarted, and one website's Cloudflare failure never stops another's tunnel `[@ANCHOR: ensure_one_tunnel_running]`. "Routes pushed yet?" lives on the tunnel record (`routes_provisioned`); installs carrying the old single `cloudflare.tunnel.provisioned` system parameter have it folded onto the one tunnel it was really about, once `[@ANCHOR: migrate_global_provisioned_flag]`.

## 3. Automated Subsystems
* **Header Injection:** Injects `Cloudflare-CDN-Cache-Control` headers via `ir.http._post_dispatch` `[@ANCHOR: ir_http_post_dispatch_headers]`. This code, the first-visit cookie rules and the CSRF refresh script live in the `edge_cache` module (a dependency of this one, usable on its own by an instance with no Redis). Dynamic and sensitive routes `[@ANCHOR: cf_nocache_routes]` are excluded.

* **Request Context Safety:** Safely extracts edge headers even in non-HTTP or unbound request contexts `[@ANCHOR: cf_get_request_context]`.
* **Boot-time Sync:** `_trigger_edge_purge_static_assets()` scans installed modules' `static/` folders and queues an `odoo-static-assets` tag purge for every website with credentials when the newest file is newer than the `cloudflare.last_static_mtime` system parameter. Nothing currently calls it at boot (only its test does), so this runs only when invoked explicitly.
* **Content Hooks:** Automatically enqueues purges for `website.page`, `blog.post`, `product.template`, and `website.menu` modifications and deletions. A record with no website is purged on every website, since it is served on all of them.

## 4. Zero-Sudo & Micro-Privilege Architecture
Strictly adheres to Zero-Sudo architecture using dedicated service accounts:
* `cloudflare.user_cloudflare_purge`: Cache purging.
* `cloudflare.user_cloudflare_waf`: WAF and IP banning, the install-time WAF sync, and Turnstile verification.
* `cloudflare.user_cloudflare_tunnel`: Tunnel management, custom hostname provisioning, and pending hostname delete retries.
* `cloudflare.user_cloudflare_trusted_ip`: Daily refresh of the trusted Cloudflare IP ranges.

## 🔐 Security & Multi-Tenancy
The module enforces website-level isolation through the website's company: record rules let a user see purge-queue, WAF-rule, IP-ban, configuration-backup and tunnel records only for websites whose company is among the user's allowed companies (or that have no company). The Cloudflare Edge menu is visible only to Settings administrators. API tokens and Zone IDs are never exposed to the frontend and are handled exclusively by backend service accounts; the API token and Turnstile secret are stored encrypted with a per-company key, and the credential fields are readable only by administrators and the Cloudflare service groups.

---

<stories_and_journeys>
## 5. Architectural Stories & Journeys

* [Asynchronous Cache Purging](docs/stories/cache_purging.md) `[@ANCHOR: story_cache_purging]`

* [Geo-Aware Request Context](docs/stories/request_context.md) `[@ANCHOR: story_request_context]`

* [Secure Edge Bridging via Tunnels](docs/stories/tunnels.md) `[@ANCHOR: story_tunnels]`

* [A custom hostname Cloudflare would not let go](docs/stories/hostname_pending_deletes.md)

* [CAPTCHA Verification with Turnstile](docs/stories/turnstile_verification.md) `[@ANCHOR: story_turnstile]`

* [Automated WAF IP Banning](docs/stories/waf_banning.md) `[@ANCHOR: story_waf_banning]`

### Journeys
* [High-Performance Content Invalidation](docs/journeys/content_invalidation.md) `[@ANCHOR: journey_content_invalidation]`

* [Managing Edge Security](docs/journeys/edge_security.md) `[@ANCHOR: journey_edge_security]`

* [Infrastructure Provisioning](docs/journeys/infrastructure.md) `[@ANCHOR: journey_infrastructure]`

* [Intelligent Traffic Handling](docs/journeys/traffic_handling.md) `[@ANCHOR: journey_traffic_handling]`
</stories_and_journeys>

## 6. External Dependencies
* `python`: `[]`
