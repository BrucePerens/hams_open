# Story: Scaling the Watchtower

## Persona
**Bob**, an Infrastructure Administrator tasked with onboarding a new microservice into the monitoring fleet.

## Context
The company has just deployed a new "Order Processing" service. Bob needs to ensure it's monitored for availability and that any crashes are reported immediately.

## The Workflow
The system was recently installed, and Bob noticed that the documentation was automatically available in the Knowledge module upon installation (see "Documentation injection" below). He used this documentation to understand the setup process.

Bob opens the Odoo backend and navigates to the Monitoring Checks view (see "The Monitoring Checks views render" below).

1.  **Creation:** He creates a new `pager.check` record of type "HTTP". He enters the URL of the order service's health endpoint. Because he picked "HTTP(S) Endpoint" rather than "Heartbeat (Push Monitor)", the form reveals the `target` URL field for him to fill in [@ANCHOR: COMM_pager_check_dynamic_invisible_http]; had he instead picked "Heartbeat (Push Monitor)", `target` would disappear and a "Heartbeat Info" notebook page would appear in its place, since a heartbeat check is pinged by the remote service rather than polled at a URL [@ANCHOR: COMM_pager_check_dynamic_invisible_heartbeat].
2.  **Configuration:** Bob sets the interval to 60 seconds and assigns the "Odoo XML-RPC Handshake" as the parent check to ensure he doesn't get flooded with order service alerts if the entire network goes down.
3.  **Helpdesk Integration:** Bob enables the Helpdesk integration so that if a check fails, a support ticket is automatically created in his department's queue [@ANCHOR: pd_helpdesk_adapter]. (For a mirrored inbound email the adapter takes the spam verdict from the context key `hams_inbound_spam_reasons`, set by `pager.incident.message_new()`; a direct call without the key judges only subject and description, and only for `email:`/`*-email:` sources, so monitoring incidents are never judged.)

4.  **Synchronization:** Bob clicks the "JSON Configuration Tools" wizard (`action_push_to_json`, described below). He reviews the generated JSON and clicks "Export to JSON". This pushes the new configuration to the daemon's persistent storage.

4.  **Verification:** The `generalized_monitor.py` daemon reloads the configuration on its next tick. Bob watches the NOC Board (`get_board_data`, described below) and sees the new "Order Processing" check appear in the green "Healthy" state.

## Success
Bob can now rest easy knowing that the new service is under the protection of the Pager Duty guardian, and he didn't have to touch a single server terminal to set it up.

## Extended Operations: Heartbeats, Autodiscovery, and Certificates

A few weeks later, Bob is asked to onboard a fleet of new, less conventional integrations.

1.  **A Push-Style Heartbeat Check:** One of the new services is a cron job that runs on a machine Bob can't open a hole in the firewall for. Instead of Pager Duty polling it, he creates a "Heartbeat (Push Monitor)" check and copies its generated `heartbeat_uuid` into the cron job's own script. Every time the job finishes, it pings `GET /api/v1/pager/heartbeat/<uuid>` (the heartbeat endpoint), which looks the check up by its UUID and stamps `last_heartbeat`. Odoo's own `ping` endpoint (`GET /api/v1/pager/ping`) answers a plain unauthenticated ping so Bob can confirm the API itself is alive before wiring up anything real. Later, `check_heartbeat_rpc` is what actually decides whether that heartbeat is still "fresh" against the check's configured interval -- if the cron job stops running, the heartbeat goes stale and the check flips unhealthy on its own, with no daemon needed on that side at all.

2.  **Cache Invalidation Underneath Him:** Every time Bob creates, edits, or deletes a `pager.check` record (`create`, `write` and `unlink`, described below), a cache-invalidation notification fires so that `generalized_monitor.py`'s in-memory config (and any other Odoo worker's cached read) picks up the change on its next cycle rather than serving stale data -- Bob never has to remember to "restart" anything after an edit.

3.  **A Password-Style API Key Field:** One of the checks needs a bearer token stored on the record. Bob notices the field renders masked, like a password, even though it's a plain `Char` field under the hood -- `_valid_field_parameter` [@ANCHOR: pager_check_valid_field_parameter] is what tells Odoo's own field-registry validation that `password=True` is a legal attribute to set on this model's fields, so the view arch can request masking without Odoo rejecting the view as malformed.

4.  **Pulling Config Back From the Daemon:** Bob makes a quick edit directly in the daemon's own JSON file during an incident (rather than the Odoo UI) and then wants Odoo to pick it back up. The "Pull from JSON" action [@ANCHOR: action_pull_from_json] reads the daemon's file and syncs it back into the `pager.check` records (it first deletes every existing check in the database and re-creates them from the file, so any check missing from the file is lost), the reverse direction of the "Export to JSON" wizard from Bob's original onboarding.

5.  **Autodiscovery:** Rather than creating each check by hand, Bob runs autodiscovery (`_run_autodiscovery` through the `action_autodiscover` button) against the network, which finds new HTTP/XML-RPC endpoints and proposes new `pager.check` records for the ones Pager Duty doesn't already know about, so Bob only has to review and approve rather than type in every URL from scratch. (Checks whose name already exists are skipped, and the JSON file is pushed to the daemon afterwards; see `action_autodiscover` in `../../README.md`.)

6.  **A Manual Trigger Button:** During a planned maintenance drill, Bob wants to fire a check on demand rather than waiting for its normal interval. Clicking "Trigger Now" [@ANCHOR: action_trigger_check] shows Bob a client notification; it does not run the check itself, because the external daemon runs checks on its own polling schedule (see `action_trigger_check` in `../../README.md`).

7.  **Parent Check Sanity:** Bob briefly fat-fingers a check's "parent check" field back onto itself while reorganizing the hierarchy. `_check_parent_check_id` [@ANCHOR: check_parent_check_id] rejects the self-reference outright with a clear validation error instead of letting a check silently suppress its own alerts by depending on itself.

8.  **Certificate and Token Expiry:** Two more guardians watch things Bob would otherwise have to remember by hand. `update_lets_encrypt_domains` keeps the certbot-managed domain list in sync with the live checks so a newly onboarded HTTPS endpoint gets covered by the renewal automation without a separate manual step. Separately, the `check_github_pat_expiry.py` daemon watches the GitHub fine-grained PAT this fleet itself uses for `hams_ticket_triage_mcp` the same way: it reads the token off disk (`_read_token`), asks GitHub's own `/user` endpoint for the token's own expiry header (a plain GET works for any fine-grained PAT regardless of scope) (`_fetch_expiry_header`), and its `main()` entrypoint pages before the token expires and silently breaks the MCP (Model Context Protocol) server's GitHub access. The expiry daemon uses one graduated severity ladder (`severity_for_days_left`) -- critical at 2 days left, high at 7, medium at 14, low at 30 -- so any expiry check escalates identically instead of hand-rolling its own thresholds, and a "synthetic" check extracts that same `[SEVERITY:...]` prefix back out of a monitor's message text (`extract_severity_prefix`) so PagerDuty's own severity field matches what the check actually meant.

9.  **Multi-Website Parent/Child Hierarchies:** Running several independent sites off one daemon, Bob gives two checks on different websites the same human-readable name on purpose (e.g. both call their frontend check "Website Up"). `check_key` [@ANCHOR: check_key] keys each check's own daemon state by its record id (falling back to name only for an older config file that predates ids), so same-named checks on different websites never collide in the in-memory state dict. `parent_suppresses` [@ANCHOR: parent_suppresses] mirrors that: a child's alerts are suppressed only by ITS OWN parent (matched by id when available), so one website's failing "Website Up" parent can never silently suppress another website's same-named, independently-failing child.

10. **Finding the Live Config File Across Deployment Styles:** The daemon resolves which JSON config file to actually read the same way the Odoo-side "Push to JSON" wizard decides where to write it: an explicit `PAGER_CONFIG_PATH` environment variable first, then `pager_config.json` under the system configuration directory if one exists there, and only then the file sitting next to the daemon script itself [@ANCHOR: daemon_resolve_config_path] -- without this, a system directory that became writable after initial setup would leave every future "Push to JSON" writing a file the daemon never actually reads, while still reporting "Export Successful" to Bob.
11. **The Log Analyzer Reads the Same Config File:** The log analyzer resolves its configuration file by the same rule (`PAGER_CONFIG_PATH`, then the system directory, then the file next to the script) [@ANCHOR: pager_duty:log_analyzer_resolve_config_path], so log patterns pushed from Odoo reach it wherever the monitor reads them. During a planned restart (`pagerduty-maintenance`) its reports are dropped with a warning instead of paging [@ANCHOR: pager_duty:log_anomaly_in_maintenance].

## Anchor reference

The functions this story walks through, one entry each. Module paths are relative to `pager_duty/`.

### Documentation injection [@ANCHOR: doc_inject_pager_duty]

At install, the module manifest's `knowledge_docs` list names one article, "Pager Duty & Generalized Monitoring", whose body is `data/documentation.html` (the heading carries the id `doc_inject_pager_duty`). `ir.module.module._bootstrap_knowledge_docs` in `zero_sudo` (run on every registry load) creates or refreshes it as a `knowledge.article` in the workspace category as the knowledge service account, so Bob finds the manual in Knowledge without any step of his own. The anchor marks that data file and the test. Arguments: none (declarative data). Errors: when the `knowledge` module is absent no article is made and nothing fails. The bootstrap remembers each article by key, so running it again updates rather than duplicates. Test: `test_03_documentation_injection` in `tests/test_pager_security.py` runs the bootstrap, then finds the article by name and checks that its body mentions Pager Duty and Generalized Monitoring.

### The Monitoring Checks views render [@ANCHOR: test_pager_view]

Marks the tests that open the views Bob uses to create and edit checks and to read incidents. `test_03_views_render` in `tests/test_generalized_config.py` calls `get_view` for the `pager.check` form and list views, and `test_04_views_render` in `tests/test_incident.py` does so for the `pager.incident` form and list and the `calendar.event` form; each result must contain an `arch`. A view naming a removed field, a bad `invisible` expression or malformed XML makes `get_view` raise and the test fail. The views in `views/*.xml` cite this anchor in an `audit-ignore-view` comment instead of having a test each. No arguments and no return value; it needs a database and runs after install.

### `action_push_to_json(self)` [@ANCHOR: generalized_pager_config]

The "Export to JSON" action of the JSON Configuration Tools (`models/pager_check.py`, `@public_rpc`). It writes the daemon's configuration file from the database. Arguments: none (called on the model). It exports every active `pager.check` (at most 1000) as a dict with the keys the daemon reads: `id`, `name`, `type`, `target`, `interval`, and, only when set, `port`, `send`, `send_hex`, `expect`, `dbname`, `user`, `password`, `query`, `script`, `rpc_method`, `rpc_params`, `regex`, `critical`, `warning`, `snmp_community`, `snmp_oid`, `partition` (when not `/`), `ignored_services`, `grace`, `max_age`, `threshold` (when above 1), `parent_id` and `parent`, `maint_start`, `maint_end`, `remediate`, `comment`, `uuid` (heartbeat), `website_id` and the sandbox fields. The file also has `log_analyzer` (active file paths and patterns) and `odoo_database`.

The path comes from `_get_config_path` (the `pager_duty.config_dir` parameter, default `/opt/hams/etc`, when writable; else the daemon directory). The file is opened with mode 0600 and then `fchmod`ed to 0600, because it holds check passwords in plain text. Returns a client notification action: success, or a sticky "Export Failed" with the path and the OS error when the write fails (the daemon then keeps its old configuration; the admin is not told it worked). The daemon reads the file once at start, so it must be restarted to see a change. Tests: `tests/test_generalized_config.py` (`test_01`, `test_06` to `test_07`), `test_04_export_carries_a_threshold_only_when_it_differs_from_one`.

### `get_board_data(self)` [@ANCHOR: pager_board_data]

The call the NOC Board makes when it opens and whenever the bus tells it to refresh (`models/incident.py`, `@public_rpc @api.model`). It takes no arguments; the website is the context `website_id` or the current website, the company is the user's current company. It runs the PostgreSQL function `pager_get_board_data(website, company)` installed by `hooks.py` and returns its JSON as a dict: `on_duty` (the person whose pager-duty calendar event covers now, else `"None"`), `active` (open and acknowledged incidents, up to 50, newest first, with `ack_name`), `resolved` (the last 10) and `stats` (check counts per status), which is how Bob's new check appears as passing. One database round trip, read only, no locks. Test: `test_01_board_data_rpc` in `tests/test_board_edge_cases.py`.

### `ping(self, **kw)` [@ANCHOR: ping]

`GET /api/v1/pager/ping` in `controllers/ping_api.py`: a public (`auth="public"`, no CSRF) liveness answer for the Odoo HTTP stack. Arguments: none used (`**kw` swallows any query string). Returns an HTTP response with `Content-Type: application/json` and body `{"status": "ok"}`. It does no database work and no authentication, so it proves only that a worker answers; the autodiscovered "WSGI HTTP Ping" check polls it. It cannot fail except by the server being down. Tests: `test_01_ping_endpoint` in `tests/test_controllers.py`.

### `heartbeat(self, hb_uuid, **kw)` [@ANCHOR: heartbeat]

`GET` or `POST /api/v1/pager/heartbeat/<hb_uuid>` in `controllers/ping_api.py` (public, no CSRF). A remote job calls it when it finishes, which is the push-style check. `hb_uuid` (`str`) is the check's `heartbeat_uuid`; knowing it is the only authentication, so treat it as a secret. The controller gets the `pager_duty.user_pager_service_internal` service id, resolves the check with `_get_check_id_by_uuid`, and, as that service user with tracking off (`mail_notrack`), sets `last_heartbeat` to the current time. Returns `200 {"status": "ok"}`, or `404 {"err": "not found"}` for an unknown uuid. `last_heartbeat` is not an export field, so a heartbeat never rewrites the daemon file. Tests: `test_01b_heartbeat_endpoint_updates_last_heartbeat_and_check_heartbeat_rpc_sees_it` and `test_01c_heartbeat_endpoint_404s_on_an_unknown_uuid` in `tests/test_controllers.py`.

### `_get_check_id_by_uuid(self, hb_uuid, override_svc_uid=None)` [@ANCHOR: get_check_id_by_uuid]

Maps a heartbeat uuid to a check id (`models/pager_check.py`, `@api.model`, cached with `@distributed_cache`, so the answer is shared across workers until the model is invalidated by a create, write or unlink). Arguments: `hb_uuid` (`str`; empty or `None` returns `False`) and `override_svc_uid` (`int` or `None`, a service user id already looked up by the caller; default the `user_pager_service_internal` id). It searches `pager.check` as that service user for the first row whose `heartbeat_uuid` matches. Returns the record id, or `False` when none matches. No writes. Because it runs as the narrow service account, an anonymous heartbeat request never needs broader rights. Tests as for `heartbeat`.

### `check_heartbeat_rpc(self, hb_uuid, interval_sec)` [@ANCHOR: check_heartbeat_rpc]

Decides whether a push-style check is still fresh (`@public_rpc @api.model`). Arguments: `hb_uuid` (`str`, the check's heartbeat uuid) and `interval_sec` (number, the longest allowed gap in seconds). It finds the check with `_get_check_id_by_uuid` and compares the age of `last_heartbeat`. Returns `True` when the last heartbeat is no older than `interval_sec`; `False` for an unknown uuid, a check that has never had a heartbeat, or a stale one. Never raises for those cases and writes nothing. The daemon's `heartbeat` check type calls it every cycle, so a cron job that stops pinging turns the check failing with no agent on that machine. Test: `test_01b_heartbeat_endpoint_updates_last_heartbeat_and_check_heartbeat_rpc_sees_it`.

### `pager.check.create(vals_list)` [@ANCHOR: pager_check_create]

Overrides `create` (`@api.model_create_multi`) so a new check reaches the daemon. After `super().create`, it calls `notify_model_invalidation(env, "pager.check")`, which tells every worker's distributed cache that cached reads of the model (for example `_get_check_id_by_uuid`) are stale, and `_schedule_config_export()`, which registers one post-commit callback per transaction that runs `action_push_to_json` as the service account. Arguments: a list of value dicts. Returns the new records. The export happens only after the commit, so a rolled-back create never reaches the file; a failed export is logged and not raised. Tests: `test_09_creating_and_configuring_a_check_rewrites_the_config_once` in `tests/test_generalized_config.py`, `test_04_create_write_unlink_all_trigger_cache_invalidation` in `tests/test_pager_check_edge_cases.py`.

### `pager.check.write(vals)` [@ANCHOR: pager_check_write]

Overrides `write`. After `super().write`, it always sends the cache invalidation, and it schedules the config export only when `vals` touches a field of `_CONFIG_EXPORT_FIELDS` (the fields written to the daemon file, plus `active`). Arguments: `vals` (dict). Returns the boolean of the parent. The distinction matters because the daemon itself writes `status`, `last_run` and `last_heartbeat` back every cycle; those are not export fields, so they do not rewrite the file or restart anything. Several changes in one transaction produce one export. Tests: `test_09` and `test_10_a_runtime_status_write_does_not_rewrite_the_config` in `tests/test_generalized_config.py`, and `test_04_create_write_unlink_all_trigger_cache_invalidation`.

### `pager.check.unlink()` [@ANCHOR: pager_check_unlink]

Overrides `unlink`. It sends the model cache invalidation first, calls `super().unlink()`, then schedules the post-commit config export, so a deleted check also disappears from the daemon's file (a check removed by a migration or from a shell used to stay in the file and keep paging, incident 39). No arguments; returns the parent's result. If the export fails afterwards the failure is logged and the delete stays committed. Tests: `test_08_unlinking_a_check_rewrites_the_daemon_config` and `test_11_a_failing_export_does_not_break_the_committed_change` in `tests/test_generalized_config.py`.

### `_run_autodiscovery(self)` [@ANCHOR: run_autodiscovery]

Builds a baseline set of checks from the host and creates the missing ones (`@api.model`). No arguments, no return value. It proposes fixed checks (system memory, CPU and load, the systemd failed-units tracker, provisioned units, IPv4 and IPv6 root DNS, the local WSGI ping with a 120 s grace), a disk check per mounted `ext4`, `xfs`, `btrfs`, `zfs` or `vfat` partition from `psutil`, and service checks (PostgreSQL, Redis, RabbitMQ, nginx, Docker) for each of those units that `systemctl list-units` shows active. A check whose name already exists is skipped, so it is safe to repeat. It ends with `action_push_to_json`. `systemctl` runs without a shell; failures reading partitions or units are logged and skipped. Also run at install when no check exists. Test: `test_02_autodiscovery` in `tests/test_generalized_config.py`.

### `action_autodiscover(self)` [@ANCHOR: action_autodiscover]

The "Auto-Discover Services" button (`@public_rpc`; the form asks for confirmation first). It calls `_run_autodiscovery` and returns a client notification action: title "Autodiscovery Complete", message that the system was scanned and the daemon configuration updated, type success. No arguments. Errors from the scan propagate as they happen. It creates checks and writes the daemon file, so it needs the rights of a pager administrator; it holds no lock. Bob reviews the created rows afterwards. Test: `test_02_autodiscovery` in `tests/test_generalized_config.py`.

### `update_lets_encrypt_domains(self, domains)` [@ANCHOR: update_lets_encrypt_domains]

Keeps the certbot readiness check pointed at the domains that need certificates (`@api.private @api.model`, so not callable over RPC; the `update_domains` endpoint in `controllers/domain_api.py` calls it after it has checked the caller's `pager_duty.domain_api_identity` in constant time and rate limited failed attempts per address). Argument: `domains` (list of `str`). It finds the first `pager.check` of type `certbot`; if none exists it creates "Let's Encrypt Certbot Readiness" (interval 86400 s) with `target` set to the comma-joined domains, otherwise it writes that target. It then runs `action_push_to_json` so the daemon sees the change. Returns nothing. It no longer creates DNS records (the service account had no right to, and the data did not exist). Tests: `test_06_update_domains_succeeds_with_the_real_identity_and_updates_the_certbot_check` and `test_06b_update_domains_does_not_touch_ham_dns_even_when_it_is_installed` in `tests/test_controllers.py`.

### `_read_token(token_path)` [@ANCHOR: read_github_pat]

In `daemon/check_github_pat_expiry.py`. Reads the GitHub token from a file: `token_path` (`str`, default `/opt/hams/etc/keys/github_pat_ticket_triage.token` or the `HAMS_GITHUB_PAT_PATH` environment variable) and returns the file's content with whitespace stripped (the file holds the bare token on one line). Raises `OSError` (missing file, permission) which `main` prints without the token and turns into exit 1. The token is a secret: it is not logged and is only handed to `_fetch_expiry_header`. Test: `test_01_healthy_token_far_from_expiry_exits_zero` in `tests/test_check_github_pat_expiry.py`.

### `_fetch_expiry_header(token)` [@ANCHOR: fetch_github_pat_expiry]

In `daemon/check_github_pat_expiry.py`. Sends `GET https://api.github.com/user` with `Authorization: Bearer <token>` and the GitHub JSON accept header (10-second timeout, through an opener that refuses redirects so the token cannot be forwarded) and returns the value of the `github-authentication-token-expiration` response header, such as `2027-09-22 07:00:00 UTC`, or `None` when the token has no expiry. `token` is a `str`. Raises `urllib.error.URLError`, `HTTPError` or `TimeoutError` on a network or authorization failure; `main` reports it and exits 1. The endpoint reveals nothing beyond the token's own expiry. Tests: `test_01_healthy_token_far_from_expiry_exits_zero`, and `test_the_github_pat_expiry_check_refuses_a_redirect` in `tests/test_credential_redirect.py`.

### `main()` of `check_github_pat_expiry.py` [@ANCHOR: github_pat_expiry_main]

The entry point of the "Synthetic Journey (Script)" check for the GitHub token. It reads the token, fetches the expiry header and compares it with now. Exit 0 prints `token verified, no expiry set` or `token healthy, expires in N days (...)`. Exit 1 prints a reason on stderr: an unreadable token file, a network failure, an unparseable date, or `SEVERITY:<level>` and `GitHub PAT expires in N days` when `severity_for_days_left` returns a level (critical at 2 days or fewer, high at 7, medium at 14, low at 30). The monitor turns that line into the incident severity. No arguments; the path comes from `HAMS_GITHUB_PAT_PATH`. Runs once per check interval, one HTTPS request. Test: `tests/test_check_github_pat_expiry.py`.

### `severity_for_days_left(days_left)` [@ANCHOR: severity_for_days_left]

Defined in `daemon/generalized_monitor.py` and imported by the expiry scripts. `days_left` is the number of days until a credential expires. It returns the severity of the first entry of `SEVERITY_THRESHOLDS_DAYS` (2 critical, 7 high, 14 medium, 30 low) with `days_left <= threshold`, or `None` when more than 30 days remain; an already expired credential gives `critical`. Pure function, no errors. Sharing the ladder means every expiry check escalates alike. Full entry in `daemon/README.md`. Test: `tests/test_check_github_pat_expiry.py`.

### `extract_severity_prefix(msg, default="high")` [@ANCHOR: extract_severity_prefix]

Defined in `daemon/generalized_monitor.py`. A synthetic script's `SEVERITY:` line reaches the monitor as a message beginning `[SEVERITY:low|medium|high|critical] `; this function removes that prefix. Arguments: `msg` (`str`) and `default` (the severity used when there is no prefix, `high`). Returns `(severity, message)`: the extracted level and the remaining text, or `(default, msg)` unchanged. The match covers the whole message including newlines. Pure function, never raises. `polling_thread` and `check_once` use it before reporting. Full entry in `daemon/README.md`. Tests: `test_22_extract_severity_prefix` and `test_22b_extract_severity_prefix_ends_at_the_end_of_the_text` in `tests/test_generalized_monitor.py`.
