# Journey: Infrastructure Provisioning

This journey covers the setup and management of edge connectivity.

## Phase 1: Global Configuration
- Administrator configures API tokens in Settings `[@ANCHOR: COMM_xpath_rendering_cf_settings]`.

## Phase 2: Tunnel Creation
- Administrator launches the Tunnel Wizard `[@ANCHOR: COMM_cf_tunnel_setup]`.
- Wizard negotiates with Cloudflare API to create a new Tunnel record.
- Configuration tokens are generated for local deployment.

## Phase 3: Lifecycle Management
- Administrator monitors tunnel status.
- Administrator uses "Sync Tunnels" `[@ANCHOR: COMM_cf_sync_tunnels]` to align Odoo records with Cloudflare state.
- Obsolete tunnels are removed via `[@ANCHOR: COMM_cf_delete_tunnel]`.
