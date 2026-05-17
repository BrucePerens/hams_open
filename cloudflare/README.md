# Cloudflare Edge Orchestration (`cloudflare`)

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).*

This module acts as the command center for your Cloudflare CDN and Web Application Firewall (WAF). It sits quietly in the background and automatically manages edge caching, security, and IP bans so you don't have to constantly log into the Cloudflare dashboard.

## 🌟 What It Does

* **Automated Static Caching & Purging:** It forces Cloudflare to aggressively cache static assets (like images, CSS, and JS) for a full year. If you update a file and restart the server, the module detects the new file timestamp during boot and automatically tells Cloudflare to purge the stale assets globally.
* **WAF Management:** You can build, backup, and deploy Cloudflare Firewall rules directly from the Odoo backend.
* **Honeypot IP Banning:** If a malicious bot triggers a silent honeypot trap on your site, this module instantly talks to Cloudflare's API and bans their IP address at the network edge.
* **Zero Trust Tunnels:** You can provision a new `cloudflared` tunnel directly from the settings menu. The module generates the tunnel via API and gives you the exact copy-paste command to run on your server.
* **Turnstile Integration:** It provides a backend validator for Cloudflare's invisible Turnstile CAPTCHA to protect your public forms.

## 🛠️ How to Set It Up

1. Drop the `cloudflare` folder into your Odoo `addons` directory.
2. Add your credentials to your server's `.env` file (or set them in the Odoo UI under **Settings > Cloudflare Edge**):
   * `CLOUDFLARE_API_TOKEN` (Requires `Zone.Cache Purge`, `Zone.Firewall Services`, and `Account.Cloudflare Tunnel` permissions)
   * `CLOUDFLARE_ZONE_ID`
   * `CLOUDFLARE_ACCOUNT_ID` (Only needed if using Zero Trust Tunnels)
3. Install the module. It will automatically apply the baseline security rules.

---

# Technical Documentation

**Context:** Technical documentation strictly for LLMs and Integrators.

## 1. Overview
Control plane for the CDN edge. Manages Cache-Tags, WAF bans, and Turnstile CAPTCHA verification to offload CPU from the Python WSGI workers.

## 2. API Interfaces
* **WAF IP Banning:** `env['cloudflare.waf'].ban_ip(...)` dynamically injects firewall rules `[@ANCHOR: cf_execute_ban]`. Automatically lifts expired bans via `[@ANCHOR: cf_action_lift_ban]`.
* **Cache Purging:** `env['cloudflare.purge.queue'].enqueue_tags(...)`. Processes asynchronous cache invalidation queues via cron `[@ANCHOR: ir_cron_process_cf_purge_queue]`. Base URLs are accurately resolved and injected via `[@ANCHOR: enqueue_urls_base_url]`.
* **Turnstile API:** `env['cloudflare.turnstile'].verify_token(...)` securely evaluates CAPTCHA handshakes against the API `[@ANCHOR: cf_turnstile_verify]`.
* **Edge Context:** `env['cloudflare.utils'].get_request_context()` (Extracts trusted IP/Geodata) `[@ANCHOR: cf_get_request_context]`.
* **Tunnel Setup:** Wizard dynamically generates the `cloudflared` execution token command for edge network bridging `[@ANCHOR: cf_tunnel_setup]`.
* **Tunnel Management:** Modules can sync existing tunnels `[@ANCHOR: cf_sync_tunnels]` and delete them `[@ANCHOR: cf_delete_tunnel]`.

## 3. Automated Subsystems
* Injects `Cloudflare-CDN-Cache-Control` headers natively via `ir.http._post_dispatch`.
* Scans module `static/` folders on boot and automatically invalidates the CDN edge via cache tags if file modifications are detected.
* **Header Injection:** Injects `Cloudflare-CDN-Cache-Control` headers `[@ANCHOR: ir_http_post_dispatch_headers]` to control edge caching behavior.
* **Settings View Injection:** Extends standard Odoo config settings to securely accept Cloudflare API tokens `[@ANCHOR: xpath_rendering_cf_settings]`.

## 4. Zero-Sudo & Micro-Privilege Architecture
This module strictly adheres to the Zero-Sudo architecture. All operations that require elevated privileges are performed using dedicated service accounts.
* `cloudflare.user_cloudflare_purge`: Used for cache purging operations.
* `cloudflare.user_cloudflare_waf`: Used for WAF management and IP banning.
* `cloudflare.user_cloudflare_tunnel`: Used for tunnel management.

**Performance Note:** In previous versions, `prefetch_fields=False` was used in `with_context` for headless mutations. This has been removed to avoid `KeyError` regressions in Odoo 19 and to ensure consistent ORM behavior, at the cost of slight prefetch cache overhead which is negligible for these infrequent operations.

---

<stories_and_journeys>
## 5. Architectural Stories & Journeys

For detailed narratives and end-to-end workflows, refer to the following:

### Stories
* [Asynchronous Cache Purging](docs/stories/cache_purging.md)
* [Geo-Aware Request Context](docs/stories/request_context.md)
* [Secure Edge Bridging via Tunnels](docs/stories/tunnels.md)
* [CAPTCHA Verification with Turnstile](docs/stories/turnstile_verification.md)
* [Automated WAF IP Banning](docs/stories/waf_banning.md)

### Journeys
* [High-Performance Content Invalidation](docs/journeys/content_invalidation.md)
* [Managing Edge Security](docs/journeys/edge_security.md)
* [Infrastructure Provisioning](docs/journeys/infrastructure.md)
* [Intelligent Traffic Handling](docs/journeys/traffic_handling.md)
</stories_and_journeys>

# Cloudflare Edge Orchestration (`cloudflare`) - API Reference

## Purpose
Acts as the control plane for the Cloudflare CDN edge. It automatically applies caching headers to outgoing HTTP responses, manages Web Application Firewall (WAF) rules, verifies invisible CAPTCHA tokens, and provides asynchronous cache invalidation.

## Python API

### `cloudflare.purge.queue`
Manages the asynchronous queue for invalidating edge cache.

#### `enqueue_urls(urls)`
Adds specific relative URLs to the purge queue.
* **Arguments:** `urls` (list of str): e.g., `['/my-page/home', '/about']`
* **Usage:**
  ```python
  svc_uid = self.env['zero_sudo.security.utils']._get_service_uid('cloudflare.user_cloudflare_purge')
  self.env['cloudflare.purge.queue'].with_user(svc_uid).enqueue_urls(['/route'])
  ```

#### `enqueue_tags(tags)`
Adds Cloudflare Cache-Tags to the purge queue for global relational purging.
* **Arguments:** `tags` (list of str): e.g., `['user_profile_123']`

### `cloudflare.waf`

#### `ban_ip(ip_address, mode='block', duration=3600, notes="Honeypot Triggered")`
Instantly instructs the Cloudflare WAF to block or challenge an IP address, AND logs the ban locally in the `cloudflare.ip.ban` model where administrators can review or lift it via the Odoo UI.
* **Arguments:**
  * `ip_address` (str): The target IP.
  * `mode` (str): `'block'`, `'challenge'`, or `'managed_challenge'`.
  * `notes` (str): Explain exactly why this was triggered (e.g., `'Classifieds Contact Honeypot'`).
* **Usage:** (Note: You do not need to use `sudo` or Service Accounts manually. The module escalates internally to allow unauthenticated public guests to trigger the honeypot safely).
  ```python
  self.env['cloudflare.waf'].ban_ip(request.httprequest.remote_addr, notes="Spam form submitted.")
  ```

### `cloudflare.turnstile`

#### `verify_token(token, remote_ip=None)`
Verifies a Cloudflare Turnstile CAPTCHA response token against the Cloudflare verification API.
* **Returns:** `True` if valid, `False` otherwise.

### `cloudflare.utils`

#### `get_request_context()`
Parses edge-injected headers (like IP, Country, City, Lat/Lon) from the current HTTP request.
* **Returns:** `dict` containing geographic and threat data.
