# Story: Geo-Aware Request Context

As a **Developer**,
I want to access geographic and threat data provided by the Cloudflare edge,
so that I can implement regional content logic or advanced security checks.

## Scenario: Regional Pricing
1. A request arrives from a visitor in France.
2. The Cloudflare edge injects `CF-IPCountry: FR`.
3. The Odoo application calls `env['cloudflare.utils'].get_request_context()`.
4. The application logic reads the country code and displays prices in Euros.

The edge header is believed only from a trusted peer (loopback, or a trusted Cloudflare IP range); otherwise the country and threat fields are `None`, so a request sent straight to the origin cannot forge them (see Edge Context in the [module README](../../README.md)).

### `get_request_context()` [@ANCHOR: COMM_cf_get_request_context]

Method of the abstract model `cloudflare.utils` (`models/edge_context.py`), `@api.private` and `@api.model`, so server code calls it and a visitor cannot over RPC. No arguments. Outside an HTTP request (cron, shell) it returns `{}`. Otherwise it returns a dict with exactly the keys below, every value a string or `None`: all values come from request headers and are not converted, so `threat_score` is `"10"`, not `10`, and the longitude and latitude are strings.

| Key | Source | Meaning |
|---|---|---|
| `ip` | `zero_sudo` trusted client IP resolver | The real client address, always present |
| `country` | `CF-IPCountry` | Two letter country code |
| `region` | `CF-Region` | Region or state name |
| `city` | `CF-IPCity` | City name |
| `postal_code` | `CF-Postal-Code` | Postal code |
| `longitude` / `latitude` | `CF-IPLongitude` / `CF-IPLatitude` | Coordinates as strings |
| `threat_score` | `CF-Threat-Score` | Cloudflare's threat score as a string |
| `as_number` | `CF-ASN` | Autonomous system number |

Trust rule: the `CF-*` headers are used only when `cloudflare.trusted_ip_utils._is_trusted_cf_peer(remote_addr)` says the connecting peer is loopback (the Tunnel case) or inside the trusted Cloudflare ranges (off unless the administrator enabled it). From any other peer every field except `ip` is `None`, so a direct request to the origin gets no forged geography. A header that Cloudflare did not send is also `None`. No side effects, no database writes, no locks. Tests: `tests/test_request_context.py`.

**Status:** Verified by the entry below.

### `test_01_get_request_context_via_loopback_tunnel_trusts_cf_headers` [@ANCHOR: COMM_test_cf_get_request_context]

Replaces the module's `request` with a mock whose peer is `127.0.0.1` and whose headers carry `CF-Connecting-IP` 1.2.3.4, country US, city New York, longitude, latitude and threat score 10. It calls `get_request_context()` and asserts `ip`, `country`, `city` and `threat_score` equal the header values. Sibling tests in the same file (`test_01b...`) prove that the same headers from a non-loopback peer are all ignored, that a peer in an allow-listed range is believed once the non-Tunnel mode is on, and that a custom range alone, without that mode, is still ignored.
