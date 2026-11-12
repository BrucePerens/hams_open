# Data structures

### `PagerBoard` [@ANCHOR: pager_duty:pager_duty_board_pager_board]

**Purpose and location.** The class has no docstring; from its definition it provides `pager_board`. Defined in `pager_duty/controllers/board.py:8` as a class; bases: `http.Controller`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `pager_board`.

**See also:** test `test_02_board_security_and_render (tests/test_controllers.py)`.

### `PagerDutyController` [@ANCHOR: pager_duty:pager_duty_domain_api_pager_duty_controller]

**Purpose and location.** The class has no docstring; from its definition it provides `update_domains`. Defined in `pager_duty/controllers/domain_api.py:17` as a class; bases: `http.Controller`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `update_domains`.

**See also:** test `test_03_update_domains_rejects_an_empty_payload (tests/test_controllers.py)`.

### `PagerLogAPI` [@ANCHOR: pager_duty:pager_duty_log_api_pager_log_api]

**Purpose and location.** The class has no docstring; from its definition it provides `search_logs`, `search_logs_poll`, `get_log_files`. Defined in `pager_duty/controllers/log_api.py:17` as a class; bases: `http.Controller`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `search_logs`, `search_logs_poll`, `get_log_files`.

**See also:** test `test_03_async_bastion_pattern (tests/test_log_analyzer.py)`.

### `PagerPingAPI` [@ANCHOR: pager_duty:pager_duty_ping_api_pager_ping_api]

**Purpose and location.** The class has no docstring; from its definition it provides `ping`, `heartbeat`. Defined in `pager_duty/controllers/ping_api.py:9` as a class; bases: `http.Controller`.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `ping`, `heartbeat`.

**See also:** test `test_file_mtime (daemon/test_freshness.py)`.

### `OdooClient` [@ANCHOR: pager_duty:pager_duty_generalized_monitor_odoo_client]

**Purpose and location.** The class has no docstring; from its definition it provides `__init__`, `reload_key`, `execute`. Defined in `pager_duty/daemon/generalized_monitor.py:78` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`, `reload_key`, `execute`.

**See also:** test `test_odoo_client_reloads_a_rotated_key (daemon/test_odoo_client_key_reload.py)`.

### `OdooClient` [@ANCHOR: pager_duty:pager_duty_pager_mcp_server_odoo_client]

**Purpose and location.** Same JSON-2 API shape as generalized_monitor.py's own OdooClient -- duplicated rather than imported, since that module is a standalone daemon script (not a package this one can cleanly import from) and the two clients carry genuinely different, independently-auditable credential scopes (broad pager_service_internal there, narrow pager_mcp_triage_service here). A record-bound method call passes `ids=[...]` as a kwarg alongside the method's own arguments -- the same convention generalized_monitor.py's own `write`/`rpc_ensure_executable` calls already use; an `@api.model` call omits it. Defined in `pager_duty/daemon/pager_mcp_server.py:57` as a class; bases: no base class.

**Fields.** none declared by this class.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `__init__`, `execute`.

**See also:** test `test_odoo_client_reloads_a_rotated_key (daemon/test_odoo_client_key_reload.py)`.

### `PagerIncident` [@ANCHOR: pager_duty:pager_duty_incident_pager_incident]

**Purpose and location.** Represents an incident detected by the monitoring system. This model is multi-tenant and multi-website, partitioned by website_id. Defined in `pager_duty/models/incident.py:40` as an Odoo model; bases: `models.Model`. (model `pager.incident`; inherits ["mail.thread"].)

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Incident ID (required=`True`; default=`"New"`; readonly=`True`; copy=`False`) |
| `website_id` | `Many2one` | Website (comodel `website`; index=`True`; ondelete=`"cascade"`) |
| `company_id` | `Many2one` | Company (comodel `res.company`; required=`True`; default=`lambda self: self.env.company`) |
| `source` | `Char` | Source (required=`True`; index=`True`) |
| `severity` | `Selection` | Severity (values `low`, `medium`, `high`, `critical`; required=`True`) |
| `description` | `Text` | Description (required=`True`) |
| `status` | `Selection` | Status (values `open`, `acknowledged`, `resolved`; default=`"open"`; index=`True`) |
| `is_escalated` | `Boolean` | Escalated (default=`False`; index=`True`) |
| `time_acknowledged` | `Datetime` | Acknowledged At (readonly=`True`) |
| `time_resolved` | `Datetime` | Resolved At (readonly=`True`) |
| `acknowledged_by_id` | `Many2one` | Acknowledged By (comodel `res.users`; readonly=`True`) |
| `mtta` | `Float` | MTTA (Minutes): Mean Time To Acknowledge (compute=`"_compute_mtta"`; store=`True`) |
| `mttr` | `Float` | MTTR (Minutes): Mean Time To Resolve (compute=`"_compute_mttr"`; store=`True`) |
| `helpdesk_ticket_id` | `Integer` | Helpdesk Ticket ID: Stores the integer ID of the generated helpdesk ticket to remain schema-agnostic. |
| `helpdesk_ticket_model` | `Char` | Ticket Model: The Odoo model used for the ticket (e.g. hams_helpdesk.ticket or helpdesk.ticket). |
| `occurrence_count` | `Integer` | Occurrences: How many times this same source has reported while an incident for it was still open/acknowledged. (default=`1`) |
| `last_occurred` | `Datetime` | Last Occurred: When this source's own report_incident() call was last received, including repeats that did not create a new incident. (default=`lambda self: fields.Datetime.now()`) |
| `window_start` | `Datetime` | Trend Window Start: When the current rolling trend-detection window for this source began. Reset to now() whenever an occurrence arrives more than TREND_WINDOW_MINUTES after this timestamp, so window_occurrence_count reflects a real burst rate, not a lifetime total (occurrence_count already covers the lifetime total). (default=`lambda self: fields.Datetime.now()`) |
| `window_occurrence_count` | `Integer` | Occurrences In Current Window: How many times this source has occurred since window_start. Distinct from occurrence_count (the incident's full lifetime total) -- this resets whenever the rolling window lapses, so it measures rate, not cumulative count. (default=`1`) |
| `trend_raised` | `Boolean` | Trend Escalation Raised: True once this source's own accumulating low/medium occurrences crossed the trend threshold and a real, paging 'Trend:' incident was raised for it. Prevents raising a new trend incident on every subsequent occurrence once one has already fired for this burst. (default=`False`) |
| `last_paged` | `Datetime` | Last Paged: When the on-duty administrator was last told about this incident (its creation, or the last re-page while it stayed open and unacknowledged). (default=`lambda self: fields.Datetime.now()`) |

**Class attributes.** `_name = "pager.incident"`; `_description = "Pager Duty Incident"`; `_inherit = ["mail.thread"]`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_compute_mtta`, `_compute_mttr`, `write`, `action_escalate_unacknowledged`, `_notify_on_duty`, `_repage_if_due`, `_raise_trend_incident`, `_joins_unit_tickets`, `_note_unit_tickets`, `report_incident`, `message_new`, `auto_resolve_incidents`, `create`, `action_acknowledge`, `get_board_data`, `mcp_list_incidents`, `mcp_get_incident_detail`, `mcp_add_note`.

**See also:** test `test_pages_under_fourteen_days (tests/test_check_auth_cert_expiry.py)`.

### `PagerDutyIncidentTicketAdapter` [@ANCHOR: pager_duty:pager_duty_incident_ticket_adapter_pager_duty_incident_ticket_adapter]

**Purpose and location.** Extends the Odoo model "pager.incident" with no new fields and the methods `create`, `action_generate_helpdesk_ticket`, `_execute_smtp_fallback`. Defined in `pager_duty/models/incident_ticket_adapter.py:46` as an Odoo model; bases: `models.Model`. (extends `pager.incident`.)

**Fields.** none declared by this class.

**Class attributes.** `_inherit = "pager.incident"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `create`, `action_generate_helpdesk_ticket`, `_execute_smtp_fallback`.

**See also:** test `test_01_bdd_json_parsing_and_db_sync (tests/test_generalized_config.py)`.

### `PagerLogPattern` [@ANCHOR: pager_duty:pager_duty_log_analyzer_pager_log_pattern]

**Purpose and location.** Regex pattern for log analysis. This model is multi-tenant and multi-website, partitioned by website_id. Defined in `pager_duty/models/log_analyzer.py:9` as an Odoo model; bases: `models.Model`. (model `pager.log.pattern`; default order "severity desc, name asc".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Pattern Name: e.g. Kernel Filesystem Corruption (required=`True`) |
| `website_id` | `Many2one` | Website (comodel `website`; ondelete=`"cascade"`) |
| `company_id` | `Many2one` | Company (comodel `res.company`; required=`True`; default=`lambda self: self.env.company`) |
| `regex` | `Char` | Regular Expression: e.g. (ext4/xfs/btrfs).*error (required=`True`) |
| `severity` | `Selection` | Severity (values `low`, `medium`, `high`, `critical`; required=`True`; default=`"high"`) |
| `active` | `Boolean` | no label or help in the source (default=`True`) |

**Class attributes.** `_name = "pager.log.pattern"`; `_description = "Log Analyzer Regex Pattern"`; `_order = "severity desc, name asc"`; `_name_uniq = models.Constraint("UNIQUE(name, website_id)", "The pattern name must be unique per website!")`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_properties_are_read_as_a_dict_and_garbage_lines_are_skipped (daemon/test_provisioned_units.py)`.

### `PagerLogFile` [@ANCHOR: pager_duty:pager_duty_log_analyzer_pager_log_file]

**Purpose and location.** Log file to be analyzed by the daemon. This model is multi-tenant and multi-website, partitioned by website_id. Defined in `pager_duty/models/log_analyzer.py:43` as an Odoo model; bases: `models.Model`. (model `pager.log.file`.) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `filepath` | `Char` | Absolute Path: e.g. /var/log/syslog (required=`True`) |
| `website_id` | `Many2one` | Website (comodel `website`; ondelete=`"cascade"`) |
| `company_id` | `Many2one` | Company (comodel `res.company`; required=`True`; default=`lambda self: self.env.company`) |
| `active` | `Boolean` | no label or help in the source (default=`True`) |

**Class attributes.** `_name = "pager.log.file"`; `_description = "Log Analyzer Target File"`; `_path_uniq = models.Constraint("UNIQUE(filepath, website_id)", "The file path must be unique per website!")`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** none.

**See also:** test `test_properties_are_read_as_a_dict_and_garbage_lines_are_skipped (daemon/test_provisioned_units.py)`.

### `PagerLogSearchJob` [@ANCHOR: pager_duty:pager_duty_log_analyzer_pager_log_search_job]

**Purpose and location.** Model description: Log Search Job State. Defined in `pager_duty/models/log_analyzer.py:62` as an Odoo model; bases: `models.TransientModel`. (model `pager.log.search.job`.) It is a transient (wizard) model: its rows are short-lived and removed by Odoo's vacuum.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`"Log Search Job"`) |
| `uuid` | `Char` | UUID (required=`True`; index=`True`) |
| `company_id` | `Many2one` | Company (comodel `res.company`; required=`True`; default=`lambda self: self.env.company`) |
| `state` | `Selection` | no label or help in the source (values `pending`, `done`, `error`; default=`"pending"`) |
| `result_payload` | `Text` | Result JSON |

**Class attributes.** `_name = "pager.log.search.job"`; `_description = "Log Search Job State"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `rpc_update_state`.

**See also:** test `test_03_async_bastion_pattern (tests/test_log_analyzer.py)`.

### `PagerCheck` [@ANCHOR: pager_duty:pager_duty_pager_check_pager_check]

**Purpose and location.** Configurable monitoring check. This model is multi-tenant and multi-website, partitioned by website_id. Defined in `pager_duty/models/pager_check.py:22` as an Odoo model; bases: `models.Model`. (model `pager.check`; default order "name asc".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Check Name / Source (required=`True`) |
| `website_id` | `Many2one` | Website (comodel `website`; ondelete=`"cascade"`) |
| `company_id` | `Many2one` | Company (comodel `res.company`; required=`True`; default=`lambda self: self.env.company`) |
| `status` | `Selection` | Status (values `never_run`, `passing`, `failing`, `maintenance`; default=`"never_run"`; readonly=`True`) |
| `last_run` | `Datetime` | Last Run (readonly=`True`) |
| `check_type` | `Selection` | Monitor Type (values `system`, `dns`, `http`, `http3`, `tcp`, `udp`, `redis`, `rabbitmq`, `xmlrpc`, `jsonrpc`, `postgres`, `log`, `ssl`, `anomaly`, `synthetic`, `certbot`, `pg_dump`, `nginx`, `logrotate`, `cloudflared`, `smtp_dryrun`, `icmp`, `heartbeat`, `docker`, `playwright`, `bash`, `executable`, `smart`, `file_absent`, `freshness`, `memcached`, `ssh`, `systemd`, `ftp`, `imap`, `pop3`, `mysql`, `snmp`, `ldap`, `ntp`, `load`; required=`True`) |
| `snmp_community` | `Char` | SNMP Community (default=`"public"`) |
| `snmp_oid` | `Char` | SNMP OID |
| `partition` | `Char` | Disk Partition: Specific mount point to check for disk usage. (default=`"/"`) |
| `ignored_services` | `Text` | Ignored Services: Comma-separated list of systemd services to ignore when monitoring all failures (e.g., 'fwupd-refresh.service'). |
| `warning_threshold` | `Integer` | Warning Threshold % |
| `target` | `Char` | Target (Host/URL/File): Prefix with ENV: to inject environment variables. |
| `port` | `Integer` | Port |
| `payload_send` | `Char` | Send Payload (String) |
| `payload_send_hex` | `Char` | Send Payload (Hex) |
| `payload_expect` | `Char` | Expect Output |
| `dbname` | `Char` | DB Name |
| `dbuser` | `Char` | Username (DB/SMTP) |
| `dbpass` | `Char` | Password (DB/SMTP) |
| `query` | `Text` | SQL Query (Returns Integer) |
| `script` | `Char` | Shell Script Command |
| `rpc_method` | `Char` | RPC Method: e.g. execute_kw |
| `rpc_params` | `Text` | RPC Params (JSON Array/Dict): e.g. ['db', 2, 'pass', 'res.partner', 'search', [[]]] |
| `regex` | `Char` | Regex Pattern |
| `critical_threshold` | `Integer` | Critical Threshold % |
| `interval` | `Integer` | Polling Interval (sec) (default=`60`) |
| `grace_period` | `Integer` | Startup Grace Period (sec): Seconds to wait after daemon startup before reporting failures. (default=`0`) |
| `max_age_seconds` | `Integer` | Maximum Age (sec): Freshness checks: the check fails when the newest record, or the last successful run, is older than this. The target says where to look: redis-zset:<key> (newest score of a sorted set), redis-key:<key> (a stored epoch or ISO time), file:<path> (modification time), unit:<name>.service (end of the last successful run of a timer-driven oneshot unit) or feed:<name>[,<name>] (the stamp a daemon keeps in the Odoo parameter ham_feed.<name>: fails when its last good data is older than this). (default=`0`) |
| `failure_threshold` | `Integer` | Failures Before Reporting: The monitor reports a failure only after the check has failed this many runs in a row. After a failed run it runs the check again within seconds instead of waiting a whole polling interval, so a transient failure is cleared quickly and a real one is reported after about a minute. 1 reports the first failure. (default=`3`) |
| `active` | `Boolean` | no label or help in the source (default=`True`) |
| `parent_check_id` | `Many2one` | Parent Check (Dependency): If parent fails, this check is suppressed. (comodel `pager.check`) |
| `maintenance_start` | `Datetime` | Maintenance Start |
| `maintenance_end` | `Datetime` | Maintenance End |
| `code_payload` | `Text` | Script Code (Python/Bash) |
| `executable_path` | `Char` | Executable Path/Name: e.g., ./my_binary |
| `executable_args` | `Char` | Executable Arguments: e.g., --check --verbose |
| `sandbox_downloads` | `Text` | Sandbox Downloads: Provide one entry per line in the format: URL, then SHA-256, then Filename. Downloaded into the sandbox before execution. |
| `sandbox_network_access` | `Selection` | Network Access: Full network access allows reaching the internet and LAN (SSRF risk). Loop-back restricts the sandbox to local interfaces. (values `loopback`, `full`; default=`"loopback"`) |
| `auto_remediate_script` | `Char` | Auto-Remediation Script: Executed locally by daemon if check fails (e.g., /opt/reboot.sh) |
| `comment` | `Text` | Comments: Multi-line comments for documentation. This will appear natively in the JSON configuration. |
| `heartbeat_uuid` | `Char` | Heartbeat UUID (default=`lambda self: str(uuid.uuid4())`; readonly=`True`) |
| `last_heartbeat` | `Datetime` | Last Heartbeat (readonly=`True`) |

**Class attributes.** `_name = "pager.check"`; `_description = "Graphical Pager Duty Check"`; `_order = "name asc"`; `_name_uniq = models.Constraint("UNIQUE(name, website_id)", "The check name must be unique per website!")`; `_uuid_uniq = models.Constraint("UNIQUE(heartbeat_uuid)", "The heartbeat UUID must be unique!")`; `_name_not_empty = models.Constraint("CHECK(LENGTH(TRIM(name)) > 0)", "The check name cannot be empty or just spaces.")`; `_interval_positive = models.Constraint("CHECK(interval > 0)", "The polling interval must be strictly greater than zero.")`; `_grace_period_non_negative = models.Constraint("CHECK(grace_period >= 0)", "The grace period cannot be negative.")`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_get_check_id_by_uuid`, `_schedule_config_export`, `create`, `write`, `unlink`, `_valid_field_parameter`, `rpc_ensure_executable`, `check_heartbeat_rpc`, `_get_config_path`, `action_pull_from_json`, `action_push_to_json`, `_run_autodiscovery`, `action_autodiscover`, `action_trigger_check`, `update_lets_encrypt_domains`, `_check_parent_check_id`.

**See also:** test `test_odoo_client_reloads_a_rotated_key (daemon/test_odoo_client_key_reload.py)`.

### `PagerSchedule` [@ANCHOR: pager_duty:pager_duty_schedule_pager_schedule]

**Purpose and location.** Extends calendar.event to support pager duty scheduling. This model is multi-tenant as it inherits from calendar.event, and we add website_id to further isolate shifts by website. Defined in `pager_duty/models/schedule.py:10` as an Odoo model; bases: `models.Model`. (extends `calendar.event`.)

| Field | Type | Meaning |
|---|---|---|
| `is_pager_duty` | `Boolean` | Is Pager Duty Shift (default=`False`; index=`True`) |
| `website_id` | `Many2one` | Website (comodel `website`; index=`True`; ondelete=`"cascade"`) |

**Class attributes.** `_inherit = "calendar.event"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `get_current_on_duty_admin`.

**See also:** test `test_01_adapter_creates_ticket_and_event (tests/test_helpdesk_adapter.py)`.

### `_NoRedirect` in `generalized_monitor.py` [@ANCHOR: pager_duty:pager_duty_daemon_generalized_monitor_no_redirect]

**Purpose and location.** Refuses every HTTP redirect for the pager monitor daemon. `urllib` copies all request headers, including the bearer key of the monitor's service account, to wherever a 3xx answer points; a redirect to another host would hand that key over. Defined in `pager_duty/daemon/generalized_monitor.py`; the module builds its private opener `_OPENER = urllib.request.build_opener(_NoRedirect)` from it and the monitor's `OdooClient.execute` sends every Odoo call through that opener.

**Definition.** `class _NoRedirect(urllib.request.HTTPRedirectHandler)` with the single method `redirect_request(self, req, fp, code, msg, headers, newurl)`, which always returns `None`.

**Behaviour.** Returning `None` makes `urllib` raise the `HTTPError` of the 3xx status instead of following it; `OdooClient.execute` lets that error out, so the caller sees the status. No attributes, no state: one shared instance, safe on any thread. The class is private to the module; the MCP server and the PAT-expiry check each define their own identical copy (unknown why they do not share one).

**Functions that touch it.** `_NoRedirect.redirect_request`, `OdooClient.execute`.

**See also:** test `test_the_monitor_odoo_client_refuses_a_redirect` in `pager_duty/tests/test_credential_redirect.py`, which asserts that the first host sees the key and a second host never receives a request.

### `_NoRedirect` in `pager_mcp_server.py` [@ANCHOR: pager_duty:pager_duty_daemon_pager_mcp_server_no_redirect]

**Purpose and location.** Refuses every HTTP redirect for the pager MCP server, so the bearer key it uses to read pager incidents cannot be forwarded to another host by a redirect. Defined in `pager_duty/daemon/pager_mcp_server.py`; the module's `_OPENER = urllib.request.build_opener(_NoRedirect)` is what its `OdooClient.execute` sends through.

**Definition.** `class _NoRedirect(urllib.request.HTTPRedirectHandler)` with the single method `redirect_request(self, req, fp, code, msg, headers, newurl)`, which always returns `None`.

**Behaviour.** The 3xx answer surfaces as an `HTTPError`; `OdooClient.execute` turns errors into a `RuntimeError` that names the status. Stateless, shared, safe on any thread; an identical copy of the handler in the monitor and in the PAT-expiry check.

**Functions that touch it.** `_NoRedirect.redirect_request`, `OdooClient.execute`.

**See also:** test `test_the_mcp_odoo_client_refuses_a_redirect` in `pager_duty/tests/test_credential_redirect.py`.

### `_NoRedirect` in `check_github_pat_expiry.py` [@ANCHOR: pager_duty:pager_duty_daemon_check_github_pat_expiry_no_redirect]

**Purpose and location.** Refuses every HTTP redirect in the GitHub token expiry check, so the personal access token it sends in its `Authorization` header cannot be forwarded to another host by a redirect from the GitHub API URL. Defined in `pager_duty/daemon/check_github_pat_expiry.py`; the module's `_OPENER = urllib.request.build_opener(_NoRedirect)` is used by `_fetch_expiry_header`.

**Definition.** `class _NoRedirect(urllib.request.HTTPRedirectHandler)` with the single method `redirect_request(self, req, fp, code, msg, headers, newurl)`, which always returns `None`.

**Behaviour.** A redirect answer raises the `HTTPError` of the 3xx status to the caller of `_fetch_expiry_header`. Stateless, shared, safe on any thread; the token itself is a secret and is never logged by this class.

**Functions that touch it.** `_NoRedirect.redirect_request`, `_fetch_expiry_header`.

**See also:** test `test_the_github_pat_expiry_check_refuses_a_redirect` in `pager_duty/tests/test_credential_redirect.py`.
