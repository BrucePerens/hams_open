# Journey: Infrastructure Provisioning

This journey covers the setup and management of edge connectivity. The full interface of each step is in [Secure Edge Bridging via Tunnels](../stories/tunnels.md).

## Phase 1: Global Configuration

### Administrator configures API tokens in Settings [@ANCHOR: COMM_xpath_rendering_cf_settings]

The Settings screen carries a "Cloudflare Edge Orchestration" block, injected into the stock settings form by an `xpath` in `views/res_config_settings_views.xml`. Per website it holds the API token (a password field, stored encrypted), the zone id and the account id, plus the WAF, tunnel and trusted-IP-range controls. Nothing below works until the token and account id are filled in. `tests/test_settings_view_block.py` proves the block compiles into the form.

## Phase 2: Tunnel Creation

### Administrator launches the Tunnel Wizard [@ANCHOR: COMM_cf_tunnel_setup]

The "Generate Setup Command" button calls `action_generate_tunnel_command()`. It needs the website's token and account id (else `UserError`), asks Cloudflare to create a tunnel with a timestamped name, fetches its run token, and opens a modal showing `cloudflared service install <token>`. The token is carried only in the action context and is never stored in the database. Two blocking Cloudflare calls; no local daemon is started by this step.

- Wizard negotiates with Cloudflare API to create a new Tunnel record. (More exactly: the tunnel is created at Cloudflare; the local `cloudflare.tunnel` record appears at the next sync.)
- Configuration tokens are generated for local deployment: the one-time install command is the only place the token is shown.

## Phase 3: Lifecycle Management
- Administrator monitors tunnel status, shown in the list from the last sync.

### "Sync Tunnels" aligns Odoo with Cloudflare [@ANCHOR: COMM_cf_sync_tunnels]

`action_sync_tunnels()` lists each website's tunnels from Cloudflare and creates or updates a local `cloudflare.tunnel` row for each (id, name, status, creation time). It is limited to the tunnel group, Settings administrators and service accounts, isolates each website in its own savepoint so one failure does not stop the rest, and never deletes a local record whose tunnel disappeared at Cloudflare.

### Obsolete tunnels are removed [@ANCHOR: COMM_cf_delete_tunnel]

`action_delete_tunnel()` deletes each selected tunnel at Cloudflare, then stops its local `cloudflared` unit and deletes the record. Failures are reported after all selected tunnels were tried: all failed raises a `UserError`, a mix returns a warning notification so the successful deletes still commit. A tunnel whose website lacks credentials is reported as a failure and left alone.
