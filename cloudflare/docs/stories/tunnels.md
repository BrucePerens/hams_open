# Story: Secure Edge Bridging via Tunnels

As a **System Administrator**,
I want to securely connect my local Odoo instance to the Cloudflare edge without opening inbound firewall ports,
so that I can minimize the attack surface of my infrastructure.

## Scenario: Setting up a Cloudflare Tunnel
1. I open the Cloudflare Settings in Odoo `[@ANCHOR: COMM_xpath_rendering_cf_settings]`.

2. I use the Tunnel Setup Wizard to create a new tunnel `[@ANCHOR: COMM_cf_tunnel_setup]`.
3. The wizard provides a pre-configured command to run on my local server. That command embeds a
   real, one-time install token, so it is never written to this wizard's own database row -- it is
   computed on the fly from the action's own context, only for the single HTTP response that opens
   the wizard right after generating it; re-opening the same wizard record later, or reading its
   table directly, never finds the plaintext token `[@ANCHOR: COMM_tunnel_wizard_command_not_persisted]`.
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

10. Whether a given tunnel's daemon is currently up is a question the daemon layer answers per tunnel `[@ANCHOR: is_tunnel_daemon_running]`, which is what lets the job skip a healthy tunnel without spending a Cloudflare API call on it every few minutes.

## Scenario: How a tunnel's daemon actually runs on this host

11. Each tunnel's `cloudflared` process is a real `systemd --user` service, not a process this
    codebase tracks in memory: before a tunnel is first started, its template unit is rendered with
    this host's own resolved `cloudflared` binary path and written into the `odoo` user's systemd
    `--user` unit directory, re-writing (and reloading) it only when the rendered content actually
    changed `[@ANCHOR: ensure_unit_installed]`.
12. Starting a tunnel writes its run token to its own `EnvironmentFile` (never onto the unit's
    command line, where any local account could read it via `/proc/<pid>/cmdline`) and runs
    `systemctl --user enable --now` on its unit -- idempotent, so the same daily job in the scenario
    above can call it on every tick with no harm to an already-running tunnel
    `[@ANCHOR: start_tunnel_daemon]`.
13. Stopping a tunnel disables its unit and removes its run-token file; stopping with no tunnel key
    at all discovers and disables every `cloudflared@*` unit currently known to systemd, rather than
    relying on an in-process registry of "tunnels this worker itself started" that a restarted
    worker would have lost `[@ANCHOR: stop_tunnel_daemon]`.
14. All of this -- running `systemctl --user ...` with the right `XDG_RUNTIME_DIR`, and writing a
    secret file atomically at `0600` inside the one directory it's allowed to live in -- is shared,
    security-sensitive plumbing `[@ANCHOR: cloudflare_systemd]`.
