# PagerDuty Monitor Daemon

This directory contains the Generalized Monitor Daemon for infrastructure incident detection and reporting.

### Functions
- **Service Monitoring**: Polls multiple protocols and services (HTTP/3, TCP, UDP, SSL, PostgreSQL, Redis, RabbitMQ, DNS, SMTP, IMAP, etc.) for availability and expected payload responses.
- **System Metrics**: Checks system disk, memory, CPU, and I/O load.
- **Auto-Provisioning**: Interacts with Odoo to provision missing binaries (`rpc_ensure_executable`) dynamically if dependencies are absent. It tries 12 times, 10 seconds apart; if a binary still cannot be provisioned, the daemon reports a critical "Daemon Boot" incident and exits.
- **Incident Reporting**: Reports incidents securely to Odoo via JSON-2 API or uses an automated SMTP fallback if the RPC connection fails. The JSON-2 API is Odoo's `/json/2/<model>/<method>` HTTP endpoint; the daemon sends the value of `ODOO_PASSWORD` as a bearer token, so the Odoo account it acts as is the owner of that key (intended to be `pager_service_internal`; `ODOO_USER` is read but not sent), and it refuses to start when `ODOO_PASSWORD` is empty. The SMTP fallback needs the `PAGER_FALLBACK_EMAIL` and `SMTP_HOST` environment variables; without them a failed report is only written to the log. If `PAGER_WEBHOOK_URL` is set, every report is also posted to that webhook.
- **Auto-Resolution**: After a failing check passes three times in a row, the daemon asks Odoo to resolve that check's open incidents.

The daemon reads its configuration file once, at startup, so restart it after pushing new checks from Odoo.

### File Structure
- `generalized_monitor.py`: The core monitoring script executing continuous checks.
- `pager_log_analyzer.py`: Tails the configured log files for the configured regex patterns and answers interactive log searches. It starts as root, chroots to `/var/log`, drops its capabilities and switches to `nobody:adm`. It talks only to Redis; `generalized_monitor.py` forwards its findings to Odoo.
- `pager_smart_spooler.py`: Run as root every 10 minutes by `pager-smart-spooler.timer`; writes `smartctl` health results to `/var/log/pager_smart_spool.json` for the `smart` check type.
- `pager_synthetic_spooler.py`: Runs as root and executes the Playwright, Sandboxed Bash and Sandboxed Arbitrary Executable checks inside a Bubblewrap (`bwrap`) sandbox, writing results to `/var/log/pager_synthetic_spool.json`.
- `pager_mcp_server.py`: A Model Context Protocol (MCP) server for AI triage, offering only `list_incidents`, `get_incident` and `add_incident_note`. It authenticates with an API key (`PAGER_MCP_API_KEY`) issued to the narrowly scoped `user_pager_mcp_triage_service` account and cannot acknowledge or resolve incidents.
- `check_github_pat_expiry.py`: A stand-alone script for a "Synthetic Journey (Script)" check that warns before an API token expires, raising the severity as the expiry date approaches.
- `*.service`, `*.timer`: systemd units for the daemons above (see `../DEPLOYMENT.md`).
