# Story: Secure Edge Bridging via Tunnels

As a **System Administrator**,
I want to securely connect my local Odoo instance to the Cloudflare edge without opening inbound firewall ports,
so that I can minimize the attack surface of my infrastructure.

## Scenario: Setting up a Cloudflare Tunnel
1. I open the Cloudflare Settings in Odoo `[@ANCHOR: COMM_xpath_rendering_cf_settings]`.

2. I use the Tunnel Setup Wizard to create a new tunnel `[@ANCHOR: COMM_cf_tunnel_setup]`.
3. The wizard provides a pre-configured command to run on my local server.
4. I can sync existing tunnels `[@ANCHOR: COMM_cf_sync_tunnels]` or delete them `[@ANCHOR: COMM_cf_delete_tunnel]` directly from the Odoo interface.

**Status:** Verified by `[@ANCHOR: COMM_test_cf_tunnel_setup]`, `[@ANCHOR: COMM_test_cf_sync_tunnels]`, and `[@ANCHOR: COMM_test_cf_delete_tunnel]`.
