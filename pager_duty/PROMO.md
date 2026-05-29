# 🛎️ Pager Duty: Enterprise-Grade SRE for Odoo

This platform handles massive, real-time data velocities. Standard ERP monitoring simply isn't enough. We built a completely generalized, standalone Site Reliability Engineering (SRE) suite that mirrors the capabilities of Datadog and Zabbix, right inside your Odoo backend.

## 🚀 Key Capabilities

### ⚡ Asynchronous & Airgapped
The monitoring engine runs as a completely isolated Python daemon outside of Odoo. If Odoo crashes, the daemon doesn't crash with it. It catches the RPC failure and immediately utilizes its **Airgapped SMTP Fallback** to email you directly. If you configure a Webhook, it instantly pings your **Slack or Discord** via ChatOps.

### 📅 Intelligent Calendar Routing
No more guessing who is on call when disaster strikes. The Pager Duty module natively integrates with Odoo's Calendar application to dynamically route alerts. By checking the "Is Pager Duty Shift" box on a calendar event, the system instantly knows who to notify. Because it relies on standard calendar mechanics, you have absolute versatility in scheduling—easily set up recurring shifts like "Every Monday at midnight," "The first Tuesday of the month," or manage complex holiday handoffs effortlessly.

### 🔎 Pre-Flight & Dry-Run Simulators
Don't wait for a 3:00 AM failure. The daemon continuously simulates critical maintenance tasks to ensure your environment is healthy:
* **Certbot Readiness:** Validates your domain's DNS routing and runs `certbot --dry-run` to prove Let's Encrypt can renew your certificate long before it actually expires.
* **PostgreSQL Backups:** Executes schema-only `pg_dump` commands to verify your backup user still has the correct disk I/O permissions.
* **Nginx Syntax:** Runs `nginx -t` to catch fatal typos in your reverse proxy config before a restart takes the whole site offline.

### 🧠 Anomaly Detection
Standard monitors check if the database is responding. We check if the data *makes sense*. Write custom SQL queries (e.g., *"Count the number of QSOs logged in the last hour"*) and set a baseline threshold. If the count drops below normal, the system alerts you to a silent anomaly.

### 🎯 Un-Cached Root DNS Lookups
Local DNS caching hides massive outages. Our DNS monitor executes `dig +trace`, bypassing your server's cache to traverse the global root servers, proving your domain delegation is perfectly intact.

### ⏱️ Intelligent Execution
* **Stochastic Jitter:** The daemon randomizes its loop start times to mathematically prevent "thundering herds" from exhausting your database connection pool.
* **Startup Grace Periods:** Tell the daemon to silently suppress alerts for 120 seconds after a reboot, giving your heavy WSGI workers time to spin up without triggering false-positive pages.
* **Watchdog Threads:** If a network socket hangs indefinitely, the main daemon watchdog detects the dead thread and physically restarts the entire process.

### 🧠 Advanced SRE Capabilities
* **Cascading Failure Suppression:** Link checks together. If the master database crashes, downstream HTTP checks automatically silence themselves to prevent an avalanche of redundant alerts.
* **Maintenance Windows:** Define precise datetime ranges to mute alarms during planned infrastructure upgrades.
* **"Dead Man's Snitch" Heartbeats:** For jobs that *should* run (like nightly backups). Provide your external scripts with a unique `/api/v1/pager/heartbeat/<uuid>` URL. If the daemon doesn't hear from the script before the TTL expires, it raises the alarm.
* **Multi-Tier Escalation:** An automated background job sweeps for `open` incidents older than 15 minutes. If your on-call operator sleeps through their page, the system automatically escalates the alert to the entire administrative group.
* **SRE Analytics:** Native tracking of Mean Time To Acknowledge (MTTA) and Mean Time To Resolve (MTTR) metrics directly on the incident records.

## 📋 Supported Monitoring Facilities, Formats & Protocols
The daemon natively understands and monitors the following layers of your infrastructure:

**Protocols & Network Layers:**
* **HTTP/HTTPS:** Validates endpoint responsiveness, HTTP status codes, and exact string matches inside returned JSON or HTML payloads.
* **HTTP/3 (QUIC):** Validates emerging cutting-edge transport protocols over UDP natively.
* **TCP Sockets:** Opens direct L4 stream connections to monitor any generic TCP port, sends raw strings or Hexadecimal payloads, and asserts exact byte responses.
* **UDP Datagrams:** Executes connectionless packet transmissions to monitor generic UDP ports and expects specific reply payloads.
* **LDAP & NTP:** Dedicated protocol handlers for monitoring directory services and time synchronization.
* **XML-RPC & JSON-RPC:** Natively serializes dictionaries and arrays, executes remote method calls, and validates the un-marshalled Python responses directly.
* **DNS:** Full-chain resolution bypassing local caches (Root -> TLD -> Authoritative).
* **SMTP:** Validates outbound email health by executing a complete dry-run TCP handshake and login sequence without sending spam.
* **SSL/TLS:** Queries live certificate data to mathematically calculate expiration windows.

**System & Infrastructure Facilities:**
* **PostgreSQL:** Verifies DB health natively via `psycopg2` or raw socket fallbacks. It securely logs in and executes `SELECT 1;` or custom SQL threshold queries for Anomaly Detection. Also simulates `pg_dump` backup processes.
* **MySQL/MariaDB:** Native client validation that logs in using credentials to execute non-destructive `SELECT 1;` queries to guarantee database responsiveness.
* **Redis:** Native client simulation. Initiates AUTH sequences and expects synchronous PONG replies.
* **RabbitMQ:** Native binary protocol simulation via direct AMQP hex handshakes.
* **Cloudflare Tunnels:** Pre-flights Zero Trust `cloudflared` daemons to ensure certificates are valid and edge connections are active.
* **Nginx:** Simulates reverse proxy reloads to catch syntax and path errors.
* **Host OS Metrics:** Utilizes `psutil` to track RAM, Disk Space, CPU Usage, IO Wait (Disk Saturation), and Hypervisor Steal.
* **ICMP Ping:** Basic network reachability using native OS utilities.
* **Docker Container Health:** Validates the native `.State.Running` status of target containers.
* **Memcached:** Direct TCP socket handshakes requesting cache `stats`.
* **SSH Handshakes:** Validates port reachability and protocol compliance.
* **Systemd Services:** Queries `systemctl is-active` to ensure host OS processes are healthy.
* **SMART Disk Health:** Anticipates hardware failures by reading from an airgapped JSON spool populated by a highly privileged root sidecar, allowing the main monitoring daemon to safely evaluate NVMe/SSD/HDD telemetry without violating its systemd sandbox.

**File Formats & Log Analysis:**
* **Log Tailing:** Spawns isolated threads to continuously tail system files (e.g., `/var/log/syslog` or Odoo application logs).
* **RegEx Pattern Matching:** Evaluates log streams against complex Regular Expressions (e.g., `FATAL|Exception|Auth failure`) to catch silent crashes instantly.
* **JSON Configuration:** The entire monitoring engine is driven by a highly readable `pager_config.json` file, which is graphically editable directly from the Odoo backend.
