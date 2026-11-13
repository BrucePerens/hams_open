# Story: Secure Edge Bridging via Tunnels

As a **System Administrator**,
I want to securely connect my local Odoo instance to the Cloudflare edge without opening inbound firewall ports,
so that I can minimize the attack surface of my infrastructure.

Each numbered step below has an entry further down, under the heading for its anchor. The tunnel model is `cloudflare.tunnel` (`models/tunnel.py`), its routes `cloudflare.tunnel.route`, the setup wizard `cloudflare.tunnel.wizard`, and the daemon plumbing `utils/cloudflare_systemd.py`.

## Scenario: Setting up a Cloudflare Tunnel
1. I open the Cloudflare Settings in Odoo.
2. I use the Tunnel Setup Wizard to create a new tunnel.
3. The wizard provides a pre-configured command to run on my local server. That command embeds a
   real, one-time install token, so it is never written to this wizard's own database row -- it is
   computed on the fly from the action's own context, only for the single HTTP response that opens
   the wizard right after generating it; re-opening the same wizard record later, or reading its
   table directly, never finds the plaintext token.
4. I can sync existing tunnels or delete them directly from the Odoo interface.

### Cloudflare Settings block [@ANCHOR: COMM_xpath_rendering_cf_settings]

`views/res_config_settings_views.xml` inherits `base.res_config_settings_view_form` and, with an `xpath` on `//form`, inserts an `app_settings_block` titled "Cloudflare Edge Orchestration" with boxes for API credentials (token as a password field, zone id, account id, all per website), WAF push and pull buttons, the tunnel box (the "Generate Setup Command" button, `action_generate_tunnel_command`) and the trusted IP range settings. The block is how an administrator reaches everything else in this story. It is plain view arch with no code; a view that failed to compile would break the whole settings screen, so `test_03_xpath_rendering` in `tests/test_settings_view_block.py` loads the compiled settings form and asserts the injected `cloudflare_edge` ids are present.

### Tunnel setup wizard [@ANCHOR: COMM_cf_tunnel_setup]

`action_generate_tunnel_command()` on `res.config.settings` (a `@public_rpc` button, one record: `ensure_one`). It takes the website selected on the settings screen (or the current website), reads its Cloudflare API token and account id, and raises `UserError` if either is missing. It then calls `create_cfd_tunnel` with the name `odoo-edge-tunnel-<unix time>` (the tunnel secret is 32 random bytes generated here and sent with the create request), and `get_cfd_tunnel_token` for the run token; each failure raises `UserError` with Cloudflare's message. It builds `cloudflared service install <token>`, creates an empty `cloudflare.tunnel.wizard` record and returns an `ir.actions.act_window` that opens it as a modal with the command in the action context. It does not create a `cloudflare.tunnel` record (the next sync does), starts no daemon, and makes two blocking Cloudflare calls. Test: `test_03_tunnel_setup` in `tests/test_cloudflare_apis.py`.

### Install command is never stored [@ANCHOR: COMM_tunnel_wizard_command_not_persisted]

`cloudflare.tunnel.wizard.command` is a non-stored computed text field (`@api.depends_context("install_command")`). Its value is the `install_command` key of the action context, or an empty string. The command contains a real one-time Cloudflare token, and a stored column would keep that secret in the database indefinitely with no redaction. By computing it from context, the token exists only in the one HTTP response that opens the wizard; reopening the transient record later, or reading its table with SQL, finds nothing. Fields: `name` (Char, default the model description) and `command` (Text, read-only, computed). Callers must not copy `command` into any stored field or log. Tests: `test_03_tunnel_setup` (renders with the context, empty without) and `test_03b_tunnel_token_not_recoverable_from_database` in `tests/test_cloudflare_apis.py`.

### Sync tunnels [@ANCHOR: COMM_cf_sync_tunnels]

`action_sync_tunnels()` is a `@public_rpc` `@api.model` method, used by the "Sync Tunnels" button. It first calls `_check_tunnel_caller_authorized`, which raises `AccessError` unless the caller is in the Cloudflare tunnel group, a Settings administrator or a service account (without this, an anonymous visitor could have triggered Cloudflare calls for every website). It then loops over up to 1000 websites and calls `_sync_tunnels_for_website` for each inside its own database savepoint, so a failure for one website is logged and the others still sync. That helper switches to the tunnel service account, skips websites without token and account id, lists the account's tunnels with `list_cfd_tunnels`, and for each one creates or updates a `cloudflare.tunnel` row (`cf_tunnel_id`, `name`, `status`, `created_at`, `website_id`). Tunnels deleted at Cloudflare are not removed locally. Returns a success notification. Blocking network calls. Test: `test_04_sync_tunnels` and `test_04a_sync_tunnels_isolates_one_websites_failure_from_the_rest` in `tests/test_cloudflare_apis.py`.

### Delete tunnels [@ANCHOR: COMM_cf_delete_tunnel]

`action_delete_tunnel()` (a `@public_rpc` button, works on one or many selected tunnels). For each tunnel it reads the website's token and account id; with either missing the tunnel is recorded as a failure. Otherwise it calls `delete_cfd_tunnel` at Cloudflare and, on success, stops that tunnel's local `cloudflared` unit (`stop_tunnel_daemon`, so no orphan keeps running against a deleted tunnel) and deletes the record. Failures are collected, not raised mid-loop, because an exception would roll the whole request back and un-delete tunnels already removed at Cloudflare. If every tunnel failed it raises one `UserError` listing them; if only some failed it returns a sticky warning notification and the successful deletions commit; with no failures it returns nothing. Test: `test_05_delete_tunnel` in `tests/test_cloudflare_apis.py`.

**Status:** Verified by the three test entries that follow.

### `test_03_tunnel_setup` [@ANCHOR: COMM_test_cf_tunnel_setup]

Patches `create_cfd_tunnel` to return `(True, "tunnel_id_123")` and `get_cfd_tunnel_token` to return `(True, "mock_token_xyz")`, gives the website a token and account id, creates a settings record and calls `action_generate_tunnel_command()`. It asserts the returned action opens `cloudflare.tunnel.wizard`, that the wizard shows the token only when the action's context is applied, and that a plain read of the same record has an empty command.

### `test_04_sync_tunnels` [@ANCHOR: COMM_test_cf_sync_tunnels]

Patches `list_cfd_tunnels` to return one tunnel `t1` named "Tunnel 1" with status `healthy`, gives the website credentials, calls `action_sync_tunnels()` and asserts a local `cloudflare.tunnel` row with that id and name now exists. It proves that a sync creates missing local records from the Cloudflare listing. A sibling test proves one broken website does not stop the others.

### `test_05_delete_tunnel` [@ANCHOR: COMM_test_cf_delete_tunnel]

Patches `delete_cfd_tunnel` to succeed and `stop_tunnel_daemon` to a mock, creates a tunnel `t1`, calls `action_delete_tunnel()` and asserts the record is gone and `stop_tunnel_daemon` was called exactly once with `"t1"`. It proves the local record and the local daemon go away together after Cloudflare confirms the delete.

## Scenario: Reviewing and Pushing Tunnel Routes
5. I open the tunnel record's own form and route list views to review its ingress rules.
6. When the daemon starts the tunnel, Odoo pushes the merged ingress configuration to Cloudflare: this tunnel's own routes plus any global route templates, sorted by sequence, with the SSH route (only if an administrator turned it on; off by default) and a mandatory catch-all rule always appended last. If that push fails (a Cloudflare API error), the daemon still starts -- basic connectivity stays up while route provisioning retries later, rather than the whole tunnel refusing to start over a routing hiccup.

### Tunnel views [@ANCHOR: COMM_cf_tunnel_views_render]

`views/tunnel_views.xml` defines the tunnel form (with its embedded route list and the "Push Routing Config" button that runs `action_push_configuration`) and the standalone route list. Both are marked in the XML as tested by this anchor instead of by a browser tour, because the form's one real action makes an external Cloudflare call. `test_06_tunnel_views_rendering` in `tests/test_ui_tours.py` loads both compiled views and asserts the form contains `action_push_configuration` and the route list contains the `hostname` column; `test_push_configuration_merges_global_and_tunnel_routes` in `tests/test_tunnel_provisioning_security.py` exercises the button's method with the Cloudflare call mocked.

### Push failure does not stop the daemon [@ANCHOR: COMM_cloudflare_tunnel_push_config_catch_all]

This anchor names the test `test_push_configuration_failure_does_not_block_daemon_start` in `tests/test_tunnel_provisioning_security.py`, which proves the contract of step 6. It creates a tunnel with credentials, patches `action_push_configuration` to raise `RuntimeError`, the run-token fetch to succeed and the daemon to be not running, then calls `_ensure_one_tunnel_running`. It asserts the outcome is `"started"`, that `start_tunnel_daemon` was called with the token and the tunnel key, and that `routes_provisioned` stayed `False`, so the next five-minute tick retries the push. In the code the push sits inside a broad `except Exception` that logs an error and carries on.

### Route order [@ANCHOR: cloudflare:COMM_tunnel_push_route_order]

6a. The order of the pushed rules is explicit. Inside `_build_ingress`, the tunnel's own routes and the global templates (routes with no tunnel) are merged and sorted by `(sequence, id)`, because `cloudflared` applies the first rule that matches. If two rules of the merged list share a sequence, a `UserError` naming the duplicated values is raised before any network call, so nothing is pushed in an undefined order. A route may name its own hostname, its path is a regular expression (anchor it with `^` and `$`; an unanchored `/ws` also matches `/ws/daemon_uplink`), and its service may be `http://`, `ws://`, `ssh://` or `http_status:`. Test: `test_push_configuration_orders_routes_and_refuses_shared_sequence` in `tests/test_tunnel_provisioning_security.py`.

### SSH route switch [@ANCHOR: cloudflare:COMM_tunnel_ssh_route_enabled]

6b. `ssh_route_enabled` (Boolean, label "Publish SSH Route", default `False`) on `cloudflare.tunnel`. When it is set and the tunnel's website has a domain, `_build_ingress` adds a rule `ssh.<domain host>` to `ssh://localhost:22` just before the catch-all. Otherwise no SSH rule is ever pushed ("Don't expose ssh", Bruce, 2026-10-03); the 1.5 migration switched it off on every existing tunnel. Leave it off unless the `ssh.` name has a DNS record and exposing it through the tunnel is intended. The field's value is read only when building the list; it is not a secret. Tests: `test_push_configuration_ssh_route_follows_tunnel_switch` and the migration test next to it in `tests/test_tunnel_provisioning_security.py`.

## Scenario: One server fronting several websites

I run hams.com, perens.com and postopen.org from the same Odoo server, each with its own Cloudflare tunnel. That is the ordinary case here, not an exotic one.

7. A background job keeps **every** tunnel that has credentials running, not just the first one.

### Ensure every tunnel is running [@ANCHOR: COMM_ensure_tunnel_running]

`action_ensure_tunnel_running()` (`@api.private`, `@api.model`) is what the scheduled action "Cloudflare: Ensure Tunnel Daemon Running" calls every 5 minutes as the tunnel service user. It searches all tunnels first (that search is also the access check for callers outside the tunnel group); with none it returns the zeroed summary at once. Otherwise it runs `_migrate_global_provisioned_flag` once, then `_ensure_one_tunnel_running` for each tunnel inside its own savepoint, so an exception in one is logged and counted as `failed` and the rest go on. It returns and logs a dict `{"started", "already_running", "skipped", "failed"}`. Each website's tunnel has its own `cloudflared` daemon, tracked under its own Cloudflare tunnel id, so one tunnel is never started twice, one whose daemon died is started again, and stopping or losing one never touches another. Tests: `test_01_every_website_s_tunnel_is_started_not_just_the_first`, `test_02_a_tunnel_already_running_is_not_restarted` and `test_09_no_tunnels_at_all_is_a_clean_no_op` in `tests/test_tunnel_multi_website.py`.

### Ensure one tunnel is running [@ANCHOR: COMM_ensure_one_tunnel_running]

8. `_ensure_one_tunnel_running(tunnel)` brings exactly one tunnel up and returns a one-word outcome: `"skipped"` (no tunnel id, or its website has no API token or account id; logged), `"already_running"`, `"started"` or `"failed"`. A tunnel that is provisioned and whose unit is active returns at once with no Cloudflare call. Otherwise it fetches the run token (`get_cfd_tunnel_token`; a failure or an empty token logs an error, never the token, and returns `"failed"`), pushes the routes if `routes_provisioned` is false (an exception is logged and swallowed, the flag stays false so the next tick retries; success sets it true), and starts the daemon with `start_tunnel_daemon(token, tunnel_key=cf_tunnel_id)`. One website's Cloudflare outage or missing token is stepped over and the others still come up. Tests: `test_03_one_failing_tunnel_does_not_stop_the_others`, `test_04_the_provisioned_marker_is_per_tunnel` and `test_08_a_tunnel_whose_website_has_no_credentials_is_skipped`.

### Migrate the global provisioned flag [@ANCHOR: COMM_migrate_global_provisioned_flag]

9. "Have this tunnel's routes been pushed yet" is recorded on the tunnel record (`routes_provisioned`). `_migrate_global_provisioned_flag()` folds the old single system-wide parameter `cloudflare.tunnel.provisioned` onto the one tunnel the old code actually used (the lowest id), exactly once: afterwards it rewrites the parameter to the sentinel `migrated-to-per-tunnel` (kept, not deleted, so a rollback does not re-provision). It reads the parameter bypassing the Redis cache, because a cached read would keep returning the old value and the migration would run every five minutes. Returns `True` when it migrated, `False` when there was nothing to do. An already-provisioned server is neither re-provisioned nor left unable to provision its other websites. Tests: `test_05_legacy_global_flag_migrates_onto_the_lowest_id_tunnel`, `test_06_the_migration_runs_once_and_is_a_no_op_afterwards` and `test_07_an_install_that_never_had_the_flag_migrates_nothing`.

### Is the tunnel daemon running [@ANCHOR: is_tunnel_daemon_running]

10. `is_tunnel_daemon_running(tunnel_key)` in `utils/cloudflare_systemd.py` returns `True` only when `systemctl --user is-active cloudflared@<tunnel_key>.service` succeeds and prints `active`. `tunnel_key` is the Cloudflare tunnel id string. It is false for an inactive unit and also false when `systemctl` itself fails or times out (15 seconds), so a broken systemd reads as "not running" and the job tries to start it. It is a local read with no side effects and no Cloudflare call, which is what lets the job skip a healthy tunnel every few minutes without spending an API call on it. Tests: `test_01` to `test_03` in `tests/test_cloudflare_systemd.py`.

## Scenario: Other sites, run as tenants of this Odoo, behind the same tunnel

perens.com, postopen.org and the parked domains are tenants of this Odoo (the `tenant_sites` and
`parking` modules, ADR 0106), and they go through the same tunnel as hams.com.

### Ingress problems hook [@ANCHOR: cloudflare:COMM_tunnel_ingress_problems]

7a. `_ingress_problems(ingress)` takes the list `_build_ingress` computed and returns a list of strings, each a reason the list must not be pushed; an empty list means fine. In this module it always returns `[]`: it is a hook that other modules extend, and `action_push_configuration` raises `UserError` ("the ingress list was not pushed: ...") and sends nothing when any reason comes back. A rule with no hostname matches every hostname, so the path rules for this site's websocket, DX (long-distance contact) firehose, band simulator, ADIF (Amateur Data Interchange Format, the log file format) and GDPR (the EU General Data Protection Regulation) export endpoints are rows scoped to `hams.com` and `*.hams.com`; `tenant_sites` refuses the push while any tenant or parked hostname exists and a rule has a path but no hostname. Test: `test_a_push_is_refused_when_an_ingress_problem_is_reported`.

### Catch-all service [@ANCHOR: cloudflare:COMM_tunnel_catch_all_service]

7b. `catch_all_service` (Char, required, default `http://localhost:8069`) is the service for the final, hostname-less rule Cloudflare requires. It must match an `http(s)` URL without spaces or `http_status:<3 digits>`, enforced when saved by a constraint that raises `ValidationError`. The default is this Odoo: every tenant and parked hostname reaches Odoo, whose request router (`tenant_sites`) serves the tenant's website, the parked page, or an uncached 404. A row is only needed for something that is not Odoo (perens.com's `/static/` goes to the read-only static file server) or to block paths at the edge as well. A value containing `:8069` makes a push log a warning and return a "Pushed with Warning" notification, because in production an nginx buffering front end (for example port 8085) protects against truncated downloads by slow clients. Test: `test_push_configuration_catch_all_service_is_per_tunnel`.

### Build ingress [@ANCHOR: cloudflare:COMM_tunnel_build_ingress]

7c. `cloudflare.tunnel._build_ingress(global_routes=None)` returns the list a push would send, without any network call, so a planned list can be compared with the live one before anything is pushed. `global_routes` is an optional recordset of routes with no tunnel (a push loop passes one prefetched search; by default it searches). One tunnel only (`ensure_one`). The result is a list of dicts with `service` and optional `hostname` and `path`: routes sorted by `(sequence, id)`, then the optional SSH rule, then `{"service": catch_all_service}`. It raises `UserError` when two rules share a sequence. Pure apart from the read queries. Test: `test_build_ingress_is_exactly_what_a_push_sends_and_needs_no_network` asserts it equals the list `action_push_configuration` sends and that nothing was pushed.

## Scenario: How a tunnel's daemon actually runs on this host

11. Each tunnel's `cloudflared` process is a real `systemd --user` service, not a process this codebase tracks in memory.

### Install the unit [@ANCHOR: ensure_unit_installed]

`_ensure_unit_installed()` renders the template unit `cloudflared@.service.template` (from `daemons/cloudflared-ffi/packaging`), replacing `{cloudflared_bin}` with this host's own resolved `cloudflared` binary (the vendored build if executable, else the one on `PATH`, else the vendored path so the failure is loud), and writes it atomically to `~/.config/systemd/user/cloudflared@.service` of the `odoo` user. It only rewrites it, and runs `systemctl --user daemon-reload`, when the rendered text differs from the file on disk. No arguments, no return value; a failed reload is logged, a failed write raises. Called by `start_tunnel_daemon`. Test: `test_15_ensure_unit_installed_renders_once_and_skips_reload_when_unchanged`.

### Start a tunnel [@ANCHOR: start_tunnel_daemon]

12. `start_tunnel_daemon(token, tunnel_key)` installs the unit if needed, writes the run token to `/opt/hams/etc/keys/cloudflared-<tunnel_key>.env` as `TUNNEL_TOKEN=<token>` (an `EnvironmentFile`, mode 0600, never on the unit's command line where `/proc/<pid>/cmdline` would expose it) and runs `systemctl --user enable --now cloudflared@<tunnel_key>.service`. Returns `True` when systemd accepted it, `False` (after logging the error) when it did not. Idempotent: enabling a running unit is a no-op, so the five-minute job can call it on every tick. A `tunnel_key` that would resolve outside the keys directory is refused with `ValueError`. Tests: `test_04`, `test_05` and `test_14_the_unit_never_puts_the_run_token_on_the_command_line`.

### Stop a tunnel [@ANCHOR: stop_tunnel_daemon]

13. `stop_tunnel_daemon(tunnel_key=None)`. With a key it runs `systemctl --user disable --now cloudflared@<key>.service` and, if that worked, removes the key's token file (kept when the unit could not be disabled, and never removed outside the keys directory). With no key it lists every `cloudflared@*.service` unit systemd knows (`list-units --all`) and disables and cleans up each, rather than relying on an in-process registry that a restarted worker would have lost. Returns nothing; failures are logged, not raised. Used by `action_delete_tunnel`. Tests: `test_06` to `test_08` in `tests/test_cloudflare_systemd.py`.

### The systemd module [@ANCHOR: cloudflare_systemd]

14. `utils/cloudflare_systemd.py` is the shared, security-sensitive plumbing for steps 11 to 13: running `systemctl --user ...` with `XDG_RUNTIME_DIR=/run/user/<uid>` set (a system-level Odoo has no login session, so the user manager needs it; `loginctl enable-linger odoo` must have been run once) and a 15 second timeout, and writing the token file atomically at `0600` (a temporary file in the same directory, `fchmod`, then rename) only inside `/opt/hams/etc/keys`, refusing sibling directories such as `keys_evil` and `..` traversals. The unit lives in the user manager's cgroup so it survives an Odoo restart, which kills everything in `odoo.service`'s cgroup. The run token does touch disk now, so a `Restart=always` unit has something to restart with. Tests: all of `tests/test_cloudflare_systemd.py`, notably `test_09` to `test_13` for the file writer.
