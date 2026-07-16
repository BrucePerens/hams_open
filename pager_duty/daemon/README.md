# PagerDuty Monitor Daemon

This directory contains the Generalized Monitor Daemon for infrastructure incident detection and reporting.

### Functions
- **Service Monitoring**: Polls multiple protocols and services (HTTP/3, TCP, UDP, SSL, PostgreSQL, Redis, RabbitMQ, DNS, SMTP, IMAP, etc.) for availability and expected payload responses.
- **System Metrics**: Checks system disk, memory, CPU, and I/O load.
- **Auto-Provisioning**: Interacts with Odoo to provision missing binaries (`rpc_ensure_executable`) dynamically if dependencies are absent.
- **Incident Reporting**: Reports incidents securely to Odoo via JSON-2 API or uses an automated SMTP fallback if the RPC connection fails.

### File Structure
- `generalized_monitor.py`: The core monitoring script executing continuous checks.
