# Jules Issues - cloudflare

This document records problems encountered during provisioning and testing of the `cloudflare` module in the Jules VM environment.

## Provisioning Issues
(None)

## Testing Issues
- `TestRequestContext.test_01_get_request_context` and `test_02_get_request_context_no_headers` failed with `RuntimeError: object is not bound` when attempting to patch `odoo.addons.cloudflare.models.edge_context.request`. This is likely because Werkzeug's `LocalProxy` (which `request` is) cannot be patched easily if it's not bound to a context.
- `TestCloudflareUITours.test_01_ip_ban_tour` failed or hung.
- `TestWafManagement.test_04_cf_action_push_waf_rules` appears to have issues as well, though the summary just mentions 2 errors total for the module loading phase.
