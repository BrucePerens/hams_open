# Story: Geo-Aware Request Context

As a **Developer**,
I want to access geographic and threat data provided by the Cloudflare edge,
so that I can implement regional content logic or advanced security checks.

## Scenario: Regional Pricing
1. A request arrives from a visitor in France.
2. The Cloudflare edge injects `CF-IPCountry: FR`.
3. The Odoo application calls `env['cloudflare.utils'].get_request_context()` `[@ANCHOR: COMM_cf_get_request_context]`.
4. The application logic reads the country code and displays prices in Euros.

The edge header is believed only from a trusted peer (loopback, or a trusted Cloudflare IP range); otherwise the country and threat fields are `None`, so a request sent straight to the origin cannot forge them (see Edge Context in the [module README](../../README.md)).

**Status:** Verified by `[@ANCHOR: COMM_test_cf_get_request_context]`.
