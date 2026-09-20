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

## Scenario: Reviewing and Pushing Tunnel Routes
5. I open the tunnel record's own form and route list views to review its ingress rules `[@ANCHOR: COMM_cf_tunnel_views_render]`.

6. When the daemon starts the tunnel, Odoo pushes the merged ingress configuration to Cloudflare: this tunnel's own routes plus any global route templates, sorted by sequence, with the SSH route and a mandatory catch-all rule always appended last `[@ANCHOR: COMM_cloudflare_tunnel_push_config_catch_all]`. If that push fails (a Cloudflare API error), the daemon still starts -- basic connectivity stays up while route provisioning retries later, rather than the whole tunnel refusing to start over a routing hiccup.

## Scenario: One server fronting several websites

I run hams.com, perens.com and postopen.org from the same Odoo server, each with its own Cloudflare tunnel. That is the ordinary case here, not an exotic one.

7. A background job keeps **every** tunnel that has credentials running, not just the first one `[@ANCHOR: COMM_ensure_tunnel_running]`. Each website's tunnel gets its own `cloudflared` daemon, tracked under its own Cloudflare tunnel id, so one tunnel is never started twice, a tunnel whose daemon died is started again, and stopping or losing one tunnel never touches another's.

8. Each tunnel is brought up on its own `[@ANCHOR: COMM_ensure_one_tunnel_running]`: a Cloudflare outage or a missing API token on one website is logged and stepped over, and the remaining websites still come up. The job reports a summary of what it started, what was already running, what it skipped and what failed.

9. "Have this tunnel's routes been pushed yet" is recorded on the tunnel record itself. A deployment that predates this and carries the old single system-wide flag has it folded onto the one tunnel that flag was actually about, exactly once `[@ANCHOR: COMM_migrate_global_provisioned_flag]`, so an already-provisioned server is neither re-provisioned nor left unable to provision its other websites.

10. Whether a given tunnel's daemon is currently up is a question the daemon layer answers per tunnel `[@ANCHOR: COMM_is_tunnel_daemon_running]`, which is what lets the job skip a healthy tunnel without spending a Cloudflare API call on it every few minutes.
