# Story: Scaling the Watchtower

## Persona
**Bob**, an Infrastructure Administrator tasked with onboarding a new microservice into the monitoring fleet.

## Context
The company has just deployed a new "Order Processing" service. Bob needs to ensure it's monitored for availability and that any crashes are reported immediately.

## The Workflow
The system was recently installed, and Bob noticed that the documentation was automatically available in the Knowledge module upon installation [@ANCHOR: doc_inject_pager_duty]. He used this documentation to understand the setup process.

Bob opens the Odoo backend and navigates to the Monitoring Checks view [@ANCHOR: test_pager_view].

1.  **Creation:** He creates a new `pager.check` record of type "HTTP". He enters the URL of the order service's health endpoint. Because he picked "HTTP(S) Endpoint" rather than "Heartbeat (Push Monitor)", the form reveals the `target` URL field for him to fill in [@ANCHOR: COMM_pager_check_dynamic_invisible_http]; had he instead picked "Heartbeat (Push Monitor)", `target` would disappear and a "Heartbeat Info" notebook page would appear in its place, since a heartbeat check is pinged by the remote service rather than polled at a URL [@ANCHOR: COMM_pager_check_dynamic_invisible_heartbeat].
2.  **Configuration:** Bob sets the interval to 60 seconds and assigns the "Odoo XML-RPC Handshake" as the parent check to ensure he doesn't get flooded with order service alerts if the entire network goes down.
3.  **Helpdesk Integration:** Bob enables the Helpdesk integration so that if a check fails, a support ticket is automatically created in his department's queue [@ANCHOR: pd_helpdesk_adapter].

4.  **Synchronization:** Bob clicks the "JSON Configuration Tools" wizard [@ANCHOR: generalized_pager_config]. He reviews the generated JSON and clicks "Export to JSON". This pushes the new configuration to the daemon's persistent storage.

4.  **Verification:** The `generalized_monitor.py` daemon reloads the configuration on its next tick. Bob watches the NOC Board [@ANCHOR: pager_board_data] and sees the new "Order Processing" check appear in the green "Healthy" state.

## Success
Bob can now rest easy knowing that the new service is under the protection of the Pager Duty guardian, and he didn't have to touch a single server terminal to set it up.

## Extended Operations: Heartbeats, Autodiscovery, and Certificates

A few weeks later, Bob is asked to onboard a fleet of new, less conventional integrations.

1.  **A Push-Style Heartbeat Check:** One of the new services is a cron job that runs on a machine Bob can't open a hole in the firewall for. Instead of Pager Duty polling it, he creates a "Heartbeat (Push Monitor)" check and copies its generated `heartbeat_uuid` into the cron job's own script. Every time the job finishes, it pings `GET /api/v1/pager/heartbeat/<uuid>` [@ANCHOR: heartbeat], which looks the check up by its UUID [@ANCHOR: get_check_id_by_uuid] and stamps `last_heartbeat`. The daemon's own uptime probe [@ANCHOR: ping] answers a plain unauthenticated ping so Bob can confirm the API itself is alive before wiring up anything real. Later, `check_heartbeat_rpc` [@ANCHOR: check_heartbeat_rpc] is what actually decides whether that heartbeat is still "fresh" against the check's configured interval -- if the cron job stops running, the heartbeat goes stale and the check flips unhealthy on its own, with no daemon needed on that side at all.

2.  **Cache Invalidation Underneath Him:** Every time Bob creates, edits, or deletes a `pager.check` record [@ANCHOR: pager_check_create] [@ANCHOR: pager_check_write] [@ANCHOR: pager_check_unlink], a cache-invalidation notification fires so that `generalized_monitor.py`'s in-memory config (and any other Odoo worker's cached read) picks up the change on its next cycle rather than serving stale data -- Bob never has to remember to "restart" anything after an edit.

3.  **A Password-Style API Key Field:** One of the checks needs a bearer token stored on the record. Bob notices the field renders masked, like a password, even though it's a plain `Char` field under the hood -- `_valid_field_parameter` [@ANCHOR: pager_check_valid_field_parameter] is what tells Odoo's own field-registry validation that `password=True` is a legal attribute to set on this model's fields, so the view arch can request masking without Odoo rejecting the view as malformed.

4.  **Pulling Config Back From the Daemon:** Bob makes a quick edit directly in the daemon's own JSON file during an incident (rather than the Odoo UI) and then wants Odoo to pick it back up. The "Pull from JSON" action [@ANCHOR: action_pull_from_json] reads the daemon's file and syncs it back into the `pager.check` records, the reverse direction of the "Export to JSON" wizard from Bob's original onboarding.

5.  **Autodiscovery:** Rather than creating each check by hand, Bob runs autodiscovery [@ANCHOR: run_autodiscovery] [@ANCHOR: action_autodiscover] against the network, which finds new HTTP/XML-RPC endpoints and proposes new `pager.check` records for the ones Pager Duty doesn't already know about, so Bob only has to review and approve rather than type in every URL from scratch.

6.  **A Manual Trigger Button:** During a planned maintenance drill, Bob wants to fire a check on demand rather than waiting for its normal interval. Clicking "Trigger Now" [@ANCHOR: action_trigger_check] runs the check immediately and shows Bob a client notification confirming it ran, without needing to touch the daemon at all.

7.  **Parent Check Sanity:** Bob briefly fat-fingers a check's "parent check" field back onto itself while reorganizing the hierarchy. `_check_parent_check_id` [@ANCHOR: check_parent_check_id] rejects the self-reference outright with a clear validation error instead of letting a check silently suppress its own alerts by depending on itself.

8.  **Certificate and Token Expiry:** Two more guardians watch things Bob would otherwise have to remember by hand. `update_lets_encrypt_domains` [@ANCHOR: update_lets_encrypt_domains] keeps the certbot-managed domain list in sync with the live checks so a newly onboarded HTTPS endpoint gets covered by the renewal automation without a separate manual step. Separately, the `check_cloudflare_token_expiry.py` daemon reads the locally-stored Cloudflare API token [@ANCHOR: read_token], asks Cloudflare's own API how much longer that token is valid [@ANCHOR: fetch_token_expiry], and its `main()` entrypoint [@ANCHOR: cloudflare_token_expiry_main] pages Bob's team well before the token actually expires and breaks DNS automation -- the same "warn before it breaks" philosophy as the rest of the monitoring fleet. A sibling daemon, `check_github_pat_expiry.py`, watches the GitHub fine-grained PAT this fleet itself uses for `hams_ticket_triage_mcp` the same way: it reads the token off disk [@ANCHOR: read_github_pat], asks GitHub's own `/user` endpoint for the token's own expiry header (a plain GET works for any fine-grained PAT regardless of scope) [@ANCHOR: fetch_github_pat_expiry], and its `main()` entrypoint [@ANCHOR: github_pat_expiry_main] pages before the token expires and silently breaks the MCP server's GitHub access. Both expiry daemons share one graduated severity ladder [@ANCHOR: severity_for_days_left] -- critical at 2 days left, high at 7, medium at 14, low at 30 -- so they escalate identically instead of each hand-rolling their own thresholds, and a "synthetic" check extracts that same `[SEVERITY:...]` prefix back out of a monitor's message text [@ANCHOR: extract_severity_prefix] so PagerDuty's own severity field matches what the check actually meant.

9.  **Multi-Website Parent/Child Hierarchies:** Running several independent sites off one daemon, Bob gives two checks on different websites the same human-readable name on purpose (e.g. both call their frontend check "Website Up"). `check_key` [@ANCHOR: check_key] keys each check's own daemon state by its record id (falling back to name only for an older config file that predates ids), so same-named checks on different websites never collide in the in-memory state dict. `parent_suppresses` [@ANCHOR: parent_suppresses] mirrors that: a child's alerts are suppressed only by ITS OWN parent (matched by id when available), so one website's failing "Website Up" parent can never silently suppress another website's same-named, independently-failing child.

10. **Finding the Live Config File Across Deployment Styles:** The daemon resolves which JSON config file to actually read the same way the Odoo-side "Push to JSON" wizard decides where to write it: an explicit `PAGER_CONFIG_PATH` environment variable first, then `pager_config.json` under the system configuration directory if one exists there, and only then the file sitting next to the daemon script itself [@ANCHOR: daemon_resolve_config_path] -- without this, a system directory that became writable after initial setup would leave every future "Push to JSON" writing a file the daemon never actually reads, while still reporting "Export Successful" to Bob.
