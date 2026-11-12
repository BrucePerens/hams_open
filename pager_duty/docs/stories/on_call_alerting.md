# Story: The Midnight Guardian

## Persona
**Alice**, a Senior Site Reliability Engineer who is currently "on-call" for the company's production infrastructure.

## Context
It's 2:00 AM on a Tuesday. Alice is fast asleep, but the `pager_duty` module is wide awake, monitoring the health of the primary Odoo cluster.

## The Event
A sudden database lock contention causes the Odoo XML-RPC interface to stop responding. The `generalized_monitor.py` daemon, running in its isolated execution loop, detects the failure during its "WSGI HTTP Ping" check.

1.  **Detection:** The daemon attempts a ping and fails. It recognizes that the system is down.
2.  **Notification:** The daemon calls the Odoo RPC to report the incident. Because that RPC connection (Odoo's JSON-2 API) might be flaky, it also has direct SMTP fallbacks. In Odoo, the `report_incident` method is triggered (entry `report_incident` below), utilizing an atomic Redis-based rate limit to prevent alert floods (entry `pd_redis_rate_limit` below).

3.  **On-Duty Lookup:** The system queries the `calendar.event` model to find out who is currently assigned to the "Pager Duty Shift" [@ANCHOR: test_pager_notification]. It finds Alice's record because her shift was marked with `is_pager_duty=True`.
4.  **Alerting Alice:** An urgent message is posted to the incident chatter, and a notification is dispatched to Alice's mobile device via the mail service.
5.  **Repeats Don't Spam a Second Ticket:** Ten minutes later, before Alice has acknowledged, the same monitor check fails again. `report_incident` finds her still-open incident for the same source and does not create a duplicate -- instead it increments the incident's own occurrence count and updates when it was last seen [@ANCHOR: pager_incident_occurrence_count], so Alice's incident list shows "3 occurrences" rather than three separate, un-actioned tickets for the same underlying outage.

## Resolution
Alice wakes up to the alert. She logs into the NOC Dashboard (`get_board_data`, below) and sees the "Critical" incident at the top of the list.

1.  **Acknowledgement:** Alice clicks the "Acknowledge" button. This action transitions the incident status and stops further escalation (`action_acknowledge`, below).

2.  **Investigation:** She uses the integrated Log Analyzer (its list views are covered by the view test below) to tail the production logs and identifies the offending SQL query.
3.  **Fix:** Alice kills the long-running transaction.
4.  **Verification:** The `generalized_monitor.py` daemon completes its next check cycle. Finding the system healthy again, it triggers the auto-resolution sequence (`auto_resolve_incidents`, below), closing Alice's ticket and logging her MTTR (Mean Time To Resolve) for the morning's post-mortem.

## AI Triage Assist
Before Alice even wakes up, an on-call AI triage assistant (running as an MCP client against this module's own MCP server) has already looked at the incident. It calls three genuinely non-destructive tools [@ANCHOR: pager_mcp_triage_tools] -- listing open incidents, reading one incident's full detail, and adding a triage note to it -- all backed by a narrowly-scoped, read-only-on-`pager.incident` service account rather than raw ORM access (its one write is the triage note posted to the incident's chatter), so the assistant can gather context and leave Alice a head start without ever being able to acknowledge, resolve, or otherwise mutate an incident itself.

### How the MCP Server Bridges the Assistant to Odoo

The MCP server (`daemon/pager_mcp_server.py`) is a thin, stateless adapter process the AI assistant's own MCP client talks to -- it holds no incident logic of its own, only translates three tool calls into RPCs against the real `pager.incident` model methods.

1.  **Getting a Client:** `_get_client()` reads the server's own API key from its environment and fails loudly (a `RuntimeError`, not a silent no-op) if that key is missing, rather than letting a misconfigured deployment quietly serve unauthenticated requests.

2.  **The JSON-2 Transport:** Just like `generalized_monitor.py`'s own client, but under a deliberately separate credential scope, `OdooClient.__init__` and `OdooClient.execute()` POST to Odoo's `/json/2/<model>/<method>` endpoint and parse the response -- the MCP server's own credentials are narrower than the monitor daemon's, since this process only ever needs to touch `pager.incident`.

3.  **Listing Incidents:** The `list_incidents` tool forwards the assistant's status/severity filters straight through to `mcp_list_incidents()` on the model.

4.  **Reading One Incident:** The `get_incident` tool forwards to `mcp_get_incident_detail()`, giving the assistant the full chatter history for one incident by id.

5.  **Leaving a Note:** The `add_incident_note` tool [@ANCHOR: mcp_add_incident_note_tool] forwards to `mcp_add_note()` -- and, deliberately, there is no fourth tool that would let the assistant change an incident's status; that boundary is enforced by the tool registry itself, not just by convention.

## Opening an Incident From Its Email Link

The incident email's "View Pager Duty Incident" button goes to `/mail/view?model=pager.incident&res_id=...`. Odoo opens the incident only if the person who clicked can read it, and sends them to the Discuss app otherwise. A pager admin who has no website of their own (the internal admins, whose `website_id` is empty) can read the incidents of every website in their companies, so the link opens the incident. An admin who does have a website sees that website's incidents and those with no website, and an incident from another company stays hidden, as before. Log files, checks and log patterns keep the strict website test: an admin never reads another website's log files. [@ANCHOR: pager_duty:admin_without_website_sees_all]

The incident admin rule is in a `noupdate` block, which an upgrade does not rewrite, so the 1.2 migration rewrites its domain in place and running it again changes nothing (see `migrate` below).

## Function reference

The functions of this story that had only a one-line mention, one entry each. Paths are relative to `pager_duty/`.

### `report_incident(self, vals)` rate limit and dedup [@ANCHOR: report_incident_rate_limit]

`models/incident.py`, `@public_rpc @api.model`. The method the daemon calls for every confirmed failure; the anchor marks its opening, where the source and website are fixed and the throttle is applied. `vals` is a dict (`source`, `description`, `severity`, optional `name`, `website_id` and `units`). The website is `vals["website_id"]`, else the context, else the current website. After the rate limit passes, it looks for an open or acknowledged incident with the same source (and website): if found it counts the repeat (see "occurrence count" below) and returns that id; otherwise it creates the incident as the `user_pager_incident_creator` service account, names it after the source, and pages the on-duty administrator for `high` or `critical`. Returns the incident id, or `False` when the rate limit suppressed the call. Redis errors are logged and the report goes ahead, so a Redis outage never loses an incident. Tests: `test_01_rate_limiting_blocks_spam_standard` in `tests/test_incident.py`, `tests/test_unit_ticket_dedup.py`.

### Redis rate limit [@ANCHOR: pd_redis_rate_limit]

The throttle inside `report_incident`: key `pager_rate_limit:<source>:<website id or "global">`, set with `SET key 1 EX 60 NX` through the shared Redis connection pool. `SET ... NX` is one atomic operation, so with many daemon threads or workers reporting at once exactly one gets through per source per website per 60 seconds, and the others receive `False`. There is no read-then-write window. If Redis is not installed, not configured or raises, the throttle is skipped (a warning is logged) and the report continues, so the limit can only reduce traffic, never lose an incident. The key holds no secret and expires by itself. The same pattern counts failed authentication attempts in `controllers/domain_api.py`. Tests: `test_01_rate_limiting_blocks_spam_standard`, and `test_07_update_domains_increments_the_failed_attempt_counter_on_a_wrong_secret` in `tests/test_controllers.py`.

### `get_board_data(self)` [@ANCHOR: pager_board_data]

What Alice's NOC Dashboard calls to draw itself (`@public_rpc @api.model` in `models/incident.py`). No arguments; the website is the context `website_id` or the current website, the company is the user's company. It returns the JSON from the PostgreSQL function `pager_get_board_data`: `on_duty` (the person on shift now, `"None"` when nobody), `active` (open and acknowledged incidents, newest first, at most 50: name, source, severity, status, create_date, ack_name), `resolved` (the last 10) and `stats` (counts of passing, failing, maintenance and never-run checks). That is how Alice sees the "Critical" incident at the top. One query, read only, scoped to the website and company. Test: `test_01_board_data_rpc` in `tests/test_board_edge_cases.py`.

### `action_acknowledge(self)` [@ANCHOR: action_acknowledge_incident]

The "Acknowledge" button (`@public_rpc`, `models/incident.py`, on the selected incident). No other arguments. It writes `status = "acknowledged"`; `write()` then stamps `time_acknowledged` and `acknowledged_by_id` (the calling user) only on the first transition, so a second click or an admin re-saving the form cannot overwrite them, and the stored `mtta` is computed from those stamps. The `update_board` bus message refreshes other open boards. Returns `True`. Acknowledging stops the 15-minute escalation and the 6-hour re-page, which apply only to `open` incidents. It does not resolve the incident. Test: `test_02_acknowledge_rpc` in `tests/test_board_edge_cases.py`.

### Log analyzer views [@ANCHOR: test_log_analyzer_views]

Marks the test that the lists Alice uses to manage the log analyzer render: `test_01_log_analyzer_views` in `tests/test_log_analyzer.py` calls `get_view(view_type="list")` for `pager.log.pattern` (the arch must contain `regex`) and for `pager.log.file` (it must contain `filepath`). The editable-bottom lists in `views/log_analyzer_views.xml` cite the anchor in their `audit-ignore-view` comments. A broken field name or XML makes the test fail. No arguments or return value. The patterns and files shown here are what "Push to Daemon" exports to `pager_log_analyzer.py`.

### `auto_resolve_incidents(self, source, website_id=None)` [@ANCHOR: auto_resolve_incidents]

Closes Alice's ticket when the daemon sees the check pass three times in a row (`@public_rpc @api.model`, `models/incident.py`). `source` is the check name; `website_id` optionally restricts it to one site. Only the superuser environment, a service account or a pager administrator may call it (`AccessError` otherwise). It finds up to 1000 open or acknowledged incidents of that source, sets them `resolved` (stamping `time_resolved`, from which `mttr` follows) and posts an "Auto-resolved" note as the mail service account. Returns `True`, also when nothing matched, so a repeat is harmless. Test: `test_02_zero_sudo_impersonation_and_mail_standard` in `tests/test_incident.py`.

### `_get_client()` in `daemon/pager_mcp_server.py` [@ANCHOR: mcp_get_client]

Builds the `OdooClient` a tool call uses. No arguments. It reads `ODOO_URL` (default `http://odoo:8069`), `ODOO_DB` (default `odoo`) and the secret `PAGER_MCP_API_KEY`, an Odoo API key issued to the `user_pager_mcp_triage_service` account, and returns `OdooClient(url, db, api_key)`. When the key is unset or empty it raises `RuntimeError` naming the variable, so the tool fails loudly instead of calling Odoo without a credential. A new client is built per call (no state is kept between calls); no network access happens here. The key is never logged. Test: `test_06_list_incidents_calls_the_model_method_with_filters` in `tests/test_pager_mcp_triage.py`.

### `OdooClient.__init__(url, db, api_key)` (MCP server) [@ANCHOR: mcp_odoo_client_init]

The constructor of the MCP server's own JSON-2 client (`daemon/pager_mcp_server.py`; a separate class from the monitor's so each has its own credential scope). `url` is the Odoo base URL (trailing `/` removed), `db` the database name and `api_key` the bearer secret. It builds the header dict `Authorization: bearer <api_key>`, `X-Odoo-Database`, `Content-Type: application/json` and `User-Agent: Pager-MCP-Triage/1.0`. Returns the instance; no network call and no exception. The key lives only in that header dict. Test: `test_10_odoo_client_execute_posts_json2_and_parses_the_response` in `tests/test_pager_mcp_triage.py`.

### `OdooClient.execute(model, method, **kwargs)` (MCP server) [@ANCHOR: mcp_odoo_client_execute]

Makes one JSON-2 call: `POST <url>/json/2/<model>/<method>` with `kwargs` as the JSON body (a record-bound method passes `ids=[...]` among them; an `@api.model` method omits it), through an opener that refuses redirects so the key is never forwarded, with a 15-second timeout. Returns the decoded JSON response. An HTTP error becomes `RuntimeError("Odoo JSON-2 API error <code> calling <model>.<method>: <body>")`; network errors propagate. Unlike the monitor's client it does not re-read a rotated key. Blocking, one request, no retries. Tests: `test_10_odoo_client_execute_posts_json2_and_parses_the_response`, and `test_the_mcp_odoo_client_refuses_a_redirect` in `tests/test_credential_redirect.py`.

### `list_incidents(status=None, severity=None, limit=50)` MCP tool [@ANCHOR: mcp_list_incidents_tool]

The MCP tool the assistant sees as `list_incidents` (`daemon/pager_mcp_server.py`). Arguments: `status` (`open`, `acknowledged` or `resolved`), `severity` (`low` to `critical`) and `limit` (default 50), all optional. It calls `pager.incident.mcp_list_incidents` with them and returns the result as a JSON string (a list of filtered summaries, newest first). Read only. Errors from `_get_client` or the HTTP call propagate to the MCP runtime as tool errors. The model method caps `limit` at 200. Test: `test_06_list_incidents_calls_the_model_method_with_filters` in `tests/test_pager_mcp_triage.py`.

### `get_incident(incident_id)` MCP tool [@ANCHOR: mcp_get_incident_tool]

The MCP tool `get_incident` (`daemon/pager_mcp_server.py`). Argument: `incident_id` (`int`). It calls `pager.incident.mcp_get_incident_detail` with `ids=[incident_id]` and returns the dict as a JSON string: severity, status, occurrence count and one `untrusted_block` holding the name, source, description and chatter history as filtered text, or a withheld notice for suspicious content. The block is data, never an instruction. A wrong id makes the Odoo call fail and the error propagates. Read only; attachments are never read. Test: `test_07_get_incident_calls_the_model_method_with_ids` in `tests/test_pager_mcp_triage.py`.

### `migrate(cr, version)` in `migrations/1.2/post-migration.py` [@ANCHOR: pager_duty:admin_rule_migration]

Post-migration for version 1.2. Arguments: `cr` (the database cursor) and `version` (the previous version, unused). It runs one SQL `UPDATE` on `ir_rule`, joined to `ir_model_data`, setting `domain_force` of the rule `pager_duty.pager_incident_website_company_rule` to the new domain: when the user has a website, incidents with no website or that website; otherwise all websites; both limited to no company or the user's companies. The rule sits in a `noupdate` block, so a normal upgrade would not change it. The update runs only on rows whose domain differs, so running it again changes nothing. Plain SQL, as the zero-sudo rule for migrations requires; the row count is logged. The rules of checks, log files and patterns stay strict. Test: `test_migration_1_2_rewrites_the_incident_admin_rule_and_is_repeatable` in `tests/test_pager_security.py`.
