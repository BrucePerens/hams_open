# ☁️ Cloudflare Edge Orchestration (`cloudflare`)

*Copyright © Bruce Perens K6BP. AGPL-3.0.*

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

---

<stories_and_journeys>
## 4. Architectural Stories & Journeys

For detailed narratives and end-to-end workflows, refer to the following:

### Stories
* [Asynchronous Cache Purging](cloudflare/docs/stories/cache_purging.md)
* [Geo-Aware Request Context](cloudflare/docs/stories/request_context.md)
* [Secure Edge Bridging via Tunnels](cloudflare/docs/stories/tunnels.md)
* [CAPTCHA Verification with Turnstile](cloudflare/docs/stories/turnstile_verification.md)
* [Automated WAF IP Banning](cloudflare/docs/stories/waf_banning.md)

### Journeys
* [High-Performance Content Invalidation](cloudflare/docs/journeys/content_invalidation.md)
* [Managing Edge Security](cloudflare/docs/journeys/edge_security.md)
* [Infrastructure Provisioning](cloudflare/docs/journeys/infrastructure.md)
* [Intelligent Traffic Handling](cloudflare/docs/journeys/traffic_handling.md)
</stories_and_journeys>
