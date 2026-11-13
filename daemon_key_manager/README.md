# Daemon Key Manager (`daemon_key_manager`)

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).*

The **Daemon Key Manager** is the centralized authority for managing Odoo API keys for external background programs (daemons). It generates secure API keys and saves them to local `.env` files, which the external programs can read. This removes the need for manual password management or insecure hardcoded credentials. It is a core part of the system's security, ensuring that background tasks have exactly the permissions they need and nothing more.

## 🚀 Quick Start: Integration API

Other modules should request daemon credentials during their installation (e.g., in a `post_init_hook`) or via a configuration wizard.

```python
def setup_daemon_credentials(env):
    # Idempotent registration and synchronous key generation
    # This call ensures the daemon is registered for 60-day rotations.
    env['daemon.key.registry'].register_daemon(
        daemon_name="My External Daemon",
        user_xml_id="my_module.my_service_account",
        env_file_path="/opt/hams/etc/keys/my_daemon.env"
    )
```

## 🛡️ Security Architecture

### Minimum Privilege Architecture
The module follows a strict "minimum privilege" policy. It uses a dedicated service account to perform its tasks, and every API key it generates belongs to a specific "Service Account" with limited rights.

**Security Principles:**
* **No Administrative Overreach:** Keys are generated specifically for the program that will use them.
* **Automatic Expiration:** Keys are set to expire in 90 days. The system rotates them every 60 days to ensure there is always a valid key.
* **Safety Fallback:** If a service account isn't configured for long-term keys, the system provides a 24-hour temporary key and logs a warning so an administrator can fix it.

### OS-Level Sandboxing
* **Strict Permissions:** `.env` files are created with `0600` (read/write only for the Odoo server process user).
* **Directory Isolation:** Parent directories are created with `0700` to prevent other users on the system from traversing into the key storage area. The key root directory (`/opt/hams/etc/keys`) itself is `0710`: the Odoo user has full access and its group may traverse and not list it, so a daemon account in that group can reach its own family's directory and cannot enumerate the others [@ANCHOR: COMM_key_root_directory_mode]. A family's key directory (`/opt/hams/etc/keys/<family>`, for the group `hamsd_<family>`) is owned by the Odoo user with that family's group at `0750`: Odoo writes, the one daemon account in the group traverses and reads, nobody else can enter [@ANCHOR: COMM_write_secure_env_file_group_directory].
* **Per-Daemon OS Group:** A registry may carry an `os_group` (`hamsd_<family>`, the OS group of the account the daemon runs as; `<family>` is the name suffix that identifies that account and group, and it also names the key directory `/opt/hams/etc/keys/<family>`). Its key file is then written `0640` with that group, so that account, and no other daemon, can read it. The Odoo user cannot `chown` to another user (it is unprivileged), only `chgrp` to a group it belongs to, so provisioning puts it in each such group; the file stays owned by Odoo, the only account that writes it, and so does the family's key directory (`/opt/hams/etc/keys/<family>`, group = the family, `0750`, re-asserted on every write, group first and mode second). A group key file may live only in its own family's directory; the registry constraint and the writer both refuse any other location [@ANCHOR: COMM_security_constraints_os_group_directory]. The file is `0600` until the group is set, so it is never group-readable under another group, and a failure (the group missing, or the server started before provisioning added it) leaves the previous key file untouched [@ANCHOR: COMM_write_secure_env_file_group]. Only a Daemon Key Manager may set the group, and the name must match `hamsd_<family>` so a registry cannot point a key at `hams_com` or any other privileged group [@ANCHOR: COMM_security_constraints_os_group]. See `docs/proposals/DAEMON_OS_ISOLATION_PLAN.md` in hams_com.
* **Path Validation:** All paths MUST start with `/opt/hams/etc/keys/`. The module strictly blocks directory traversal (`..`) and symlink attacks by resolving the `os.path.realpath` of the requested path before performing any file operations [@ANCHOR: COMM_security_constraints_path].

To further protect the integrity of the host system, additional safety mechanisms are enforced across the platform.

* **System Directory Protection:** Writing to sensitive system directories is explicitly forbidden regardless of the prefix check. `_write_secure_env_file(path, login, key, group=None)` resolves the path with `os.path.realpath` and refuses (`UserError`) anything that does not start with `/opt/hams/etc/keys/`, even if the registry row predates the constraint. It writes through a fresh `0600` temporary file that is renamed over the target, so an existing file with loose permissions can never receive a live key and a failed write leaves the previous key in place; with a `hamsd_<family>` group the file ends `0640` with that group. Permission and OS errors are logged and re-raised [@ANCHOR: COMM_write_secure_env_file_logic].

### Automated Key Rotation
Keys are automatically rotated every 60 days via an `ir.cron` job. The scheduled action `ir_cron_rotate_daemon_keys` (`data/cron.xml`, `noupdate`) runs daily as the manager service account and calls `_cron_rotate_all_keys()`; that method rotates only registries whose key is older than 59 days, ten at a time, and re-triggers the cron while a full batch was found, so a daily run is a check and not a rotation of everything [@ANCHOR: COMM_cron_rotation_trigger].

* **Graceful Failure:** Stateless batching (processing 10 records at a time and re-triggering) ensures that one failed file-write or database error does not block other rotations. Failures are logged, and the system attempts to continue.
* **Buffer Period:** New keys are generated with a 90-day expiration, providing a 30-day "grace period" for the 60-day rotation cycle to succeed.
* **Rotation does not restart daemons, and cannot.** Odoo has no privilege to run `systemctl`, and a blind restart would also kill a running 50-minute pgbackrest backup. A rotation revokes the old key and rewrites the key file, so every long-running daemon must re-read its key file after an HTTP 401 and retry once. Ones that do: every daemon built on `hams_config.get_odoo_client()` (hams_com `daemons/hams_config.py`, `execute()`), everything built on `zero_sudo/daemon/json_rpc_client.py`, `backup_management/daemon/main.py` (`_reload_odoo_key`), `pager_duty/daemon/generalized_monitor.py` (`OdooClient.reload_key`), and `hams_helpdesk/daemon/exception_reporter.py` (reads the file on every report). A new daemon that keeps its key in memory must do the same, and `ROTATED_KEY_RELOAD_POLICY` in `hams_shared/tools/infrastructure.py` fails a test until it is declared (see the audit below). Found 2026-11-05: the backup worker kept the old key for nine days after the 2026-10-27 rotation (ticket 116).
* **Self-Healing Daemons:** Daemons utilizing these keys MUST be designed to catch `AccessError` responses from Odoo, re-read their assigned `.env` file from the disk, and retry the request.

---

## 🛠️ Technical Reference

### 1. Storage & Orchestration Mandate
All credentials **MUST** be written to `/opt/hams/etc/keys/`.
In containerized/orchestrated environments:
* **Odoo Container:** Mount the volume as **Read/Write**.
* **Daemon Containers:** Mount the volume as **Read-Only**.

### 2. Core API Methods (Public)

#### `register_daemon(daemon_name, user_xml_id, env_file_path)`
* **`daemon_name`**: A unique string identifier for the external service.
* **`user_xml_id`**: The XML ID of the service account record (e.g., `pager_duty.user_pager_service_internal`). Must have `is_service_account = True`.
* **`env_file_path`**: Absolute path where the `.env` file should be written. Must reside within `/opt/hams/etc/keys/`.
* **Behavior**: Idempotent. Immediately triggers key generation and writes the file. Associates registry with the service account's company.

#### `unregister_daemon(daemon_name)` [@ANCHOR: daemon_key_manager:unregister_daemon]
* **Use Case**: A module stops using a daemon key (the cache manager's was never used by its daemon). Call it from the module's migration instead of leaving a live credential that is rotated for nobody.
* **Arguments**: `daemon_name` (`str`): the registry row's name, exactly as given to `register_daemon`. No value is special; a name with no row is not an error.
* **Returns**: `int`, the number of registry rows removed (0 or 1: names are unique per company). **Raises**: `AccessError` unless the caller is a Daemon Key Manager, an administrator or the superuser (`distributed_redis_cache`'s migration runs as the Daemon Key Manager service account).
* **Behavior**: Revokes the API keys named `<daemon_name>_key` of the row's service account (the name `_rotate_key_and_write_file` gives them), then deletes the row, in the caller's transaction (nothing commits). Idempotent. It leaves the service account and the key file on disk: the registry never deletes a file once its row is gone, so an operator removes it. Runs as the key manager's service account (`user_daemon_key_manager_service`), not as the superuser. Not asynchronous, takes no lock, and does not rotate any other row.
* **Tests**: `tests/test_unregister_daemon.py` (a manager removes the row and the key, a second call removes nothing, a service account is refused).

#### `action_rotate_key()`
* **Use Case**: Manually rotate the key for a specific daemon via UI or code.
* **Behavior**: Revokes existing key and generates a new one synchronously.
* **Security**: Only accessible to members of `Daemon Key Management / Manager`.

#### `rotate_own_key(daemon_name, current_key)` (remote self-rotation)
* **Use Case**: A daemon that runs on another machine than Odoo, whose key file Odoo cannot write
  (e.g. a sync daemon moved to a separate machine for a non-datacenter egress path). A manager sets **Remote Self-Rotation** on its registry; the
  cron, `action_force_provision_all()` and `action_rotate_key()` then leave that registry alone.
* **Behavior**: Called over JSON-2 by the daemon itself, authenticated with its current key, which it
  also passes as `current_key`. Two phases: with the active key, once the key is more than 59 days old,
  it returns `{"status": "issued", "login", "key"}` and revokes nothing; with that new key it returns
  `{"status": "confirmed"}`, revokes the old key and writes the new one to `env_file_path`. Otherwise
  `{"status": "not_due", "next_rotation"}`. The daemon calls it at the start of each run and writes the new key to its own key file
  atomically between the two calls.
* **Security**: Only the registry's own service account, presenting a live key of that registry.

#### `action_force_provision_all()`
* **Use Case**: Used during system bootstrapping (e.g., via systemd or Kubernetes init containers) or emergency rotations.
* **Shell Invocation**:
  ```bash
  odoo-bin shell -d hams --no-http -e "env['daemon.key.registry'].action_force_provision_all(); env.cr.commit()"
  ```

### 3. Core Internal Methods (For Developers & AIs)

#### `_rotate_key_and_write_file(pre_fetched_keys=None)`
* **Behavior**: The underlying mechanism that revokes old keys via `res.users.apikeys`, generates a new 90-day key, and calls `_write_secure_env_file`. Handles validation of `__system__` restrictions.

#### `_write_secure_env_file(path, login, key, group=None)`
* **Behavior**: Safely writes the `.env` file. Enforces `0600` on the file (or `0640` with the given `hamsd_<family>` group) and `0700` on parent directories (`0710` on the key root, `0750` with the family group on a family's key directory). Prevents path traversal.

#### `_cron_rotate_all_keys()`
* **Behavior**: Triggered by cron. Uses a batch limit of 10 and triggers itself recursively to avoid transaction timeouts. Commits successful writes immediately and rolls back individual failures.

### 4. File Format (.env)
```env
# Auto-generated by daemon.key.registry
ODOO_RPC_LOGIN=service_account_login
ODOO_RPC_KEY=12345abcd...
```

---

## Rotation safety audit 2026-11-05

**Why.** A rotation writes the new key to the daemon's key file, revokes the old key, and restarts nothing. `backup.worker` read its key once
from its `EnvironmentFile=` and failed every Odoo call with 401 for nine days (2026-10-27 to 2026-11-05, ticket 116). Bruce asked that no
other daemon have the same bug. Every one of the 32 registered keys was traced to the processes that read it, in `hams_com`, `hams_open`
and the units of `hams_shared/tools/infrastructure.py` (`MANIFEST`), and the units running on hams1 were checked with
`systemctl show` (`Type`, `EnvironmentFiles`, `ExecStart`).

**What rotates, and when.** The monthly cron (`_cron_rotate_all_keys`, commits after each registry), and **every start of
`hams.daemon.keys.service`**, which calls `action_force_provision_all()`: that also revokes and re-mints every key that is not remote
self-rotating (it commits once, at the end of the shell command). So a provision or a boot rotates all keys, not only the monthly cron. In
both paths the key file is written before the transaction commits. Nothing running is restarted.

**Verdict classes.** *One-shot*: a `Type=oneshot` unit (every timer-driven service on hams1 is one) reads the file afresh at each run, so a
rotation between runs cannot hurt; a long one (`callbook.geo.enrich`, up to 31 minutes) is also covered by the reload below. *Reload*: a
long-running process whose Odoo client re-reads the key file after a 401 and retries once. *Per call*: a long-running process that makes a
new client (a new read of the key file) for every use. *Unused*: the key is loaded and never sent to Odoo. *BUG*: a long-running process that
keeps the key it read at start; there were two, both fixed before this audit finished (below). Result: **no process remains in the BUG
class.**

`hams_config.get_odoo_client()` (hams_com `daemons/hams_config.py`) is the shared reload: `execute()` treats a 401 ("Unauthorized"),
"Invalid apikey" or an AccessError as a rotation, calls `load_credentials()` again (the key file named by `ODOO_KEY_FILE`),
re-authenticates and sends the call once more. Test: `test_an_unauthorized_response_triggers_a_key_reload_and_successful_retry`.
The hams1 copy of `hams_config.py`, `backup_worker/main.py`, `generalized_monitor.py` and `exception_reporter.py` has the same sha256 as
main (checked 2026-11-05), and every `get_odoo_client()` daemon was restarted at 18:34 UTC that day, so the reload is running.

| # | Key (registry name) | Key file under `/opt/hams/etc/keys/` | Process (unit) | Unit type | How it reads the key | Verdict | Fix |
|---|---|---|---|---|---|---|---|
| 1 | Ham Radio Activator Reference Data Sync Service | `activator_sync/activator_data_service_internal.key` | `pota.sync`, `sota.sync` | oneshot, timer | file at start, `get_odoo_client()` reload | safe: one-shot | - |
| 2 | AI Safety Reporting | `ai_safety_reporting_service_internal.env` | `hams.simulated.bots`, `hams.simulated.observer` | simple, `Restart=always` | `ODOO_KEY_FILE`, a new `get_odoo_client()` for each safety ticket (`create_safety_ticket`, `_create_observer_ticket`); the observer keeps failed flags in a retry queue | safe: per call | - |
| 3 | AI Ticket Triage Service | `ai_triage_service_internal.key` | `run-ticket-triage-mcp.sh` -> `hams_ticket_triage_mcp` (stdio MCP server for one agent session, started by the `ticket.triage` one-shot) | process lives one session | a new `get_odoo_client()` per tool call | safe: per call + reload | - |
| 4 | Ham Radio APRS Sync Service | `aprs_sync/aprs_sync_service_internal.key` | `aprs.is.sync` | simple, `Restart=always` | `get_odoo_client()` at each (re)connect to APRS-IS, held for the connection, reload on 401 | safe: reload | - |
| 5 | Ham Radio Callbook Australia PII Sync Service | `au_pii/au_pii_sync_service_internal.key` | `au.pii.sync` | oneshot, timer | file at start, reload | safe: one-shot | - |
| 6 | Backup Worker RabbitMQ Consumer | `backup_worker.env` | `backup.worker` | simple, `Restart=always`; `EnvironmentFile=` only seeds the first key | **was** the environment at start; now `_reload_odoo_key()` re-reads the file after a 401 | **was BUG**, fixed | hams_open #1204 |
| 7 | Redis Cache Manager | `cache_manager.env` | none (`cache-manager` no longer loads it) | simple, `Restart=always`, running since 2026-10-12 | `cache_manager.py` has no HTTP client and never used `ODOO_RPC_KEY` (only its PostgreSQL login from `cache_manager_db.env`) | safe: unused; **no longer registered** (`distributed_redis_cache` 1.1 migration removes the row and revokes the key) | - |
| 8 | Ham Radio Callbook Geo-Enrichment Service | `callbook_geo/callbook_geo_service_internal.key` | `callbook.geo.enrich` | oneshot, timer | file at start, reload mid-run | safe: one-shot | - |
| 9 | Club Crawl Service | `club_crawl/club_crawl_service_internal.key` | `club.crawl` | oneshot, timer | file at start, reload | safe: one-shot | - |
| 10 | Code Review Sweep Service | `code_review_sweep_service_internal.key` | `code.review.sweep` | oneshot, timer | file at start, reload | safe: one-shot | - |
| 11 | Ham Radio Callbook Sync Service | `country_sync/callbook_sync_service_internal.key` | `au.acma.sync`, `au.callsign.sync`, `br.anatel.sync`, `de.bnetza.sync`, `nz.rsm.sync`, `uk.ofcom.sync`, `ised.canada.sync`, `fcc.uls.sync` (hams1) | oneshot, timer | file at start, reload | safe: one-shot | - |
| 12 | Ham Radio Callbook DNS Export Service | `dns_export/callbook_dns_export_service_internal.key` | `callbook.dns.export`, `callbook.delta.export` | oneshot, timer | file at start, reload | safe: one-shot | - |
| 13 | Ham Radio DNS API Service | `dns_sync/dns_api_service_internal.key` | `pdns.sync` | simple, `Restart=always` | `get_odoo_client()` at start, reload on 401 (its PowerDNS key is a different credential) | safe: reload | - |
| 14 | Ham DX Daemon Service | `dx_daemon/dx_daemon_service.key` | `ham.dx.daemon`; `skimmer.list.sync` | simple, `Restart=always`; oneshot, timer | `get_odoo_client()` at start (30 s call timeout), reload on 401; file at start | safe: reload; one-shot | - |
| 15 | AI Event Enrichment Service | `event_ai_correction_service_internal.key` | `event.ai.enrichment`; `run-event-enrichment-mcp.sh` -> `hams_event_enrichment_mcp` | oneshot, timer; MCP (Model Context Protocol) process lives one session | file at start; a new `get_odoo_client()` per tool call | safe: one-shot; per call | - |
| 16 | Ham Radio Event Sync Service | `event_sync/event_sync_service_internal.key` | the nine `event_sync` units (`wa7bnm.contest`, `arrl.hamfests`, `event.cover.image`, `rac.events`, `sm3cer.contest`, `electronicsfleamarket`, `hamstudy.sessions`, `radio.history.events`, `monthly.events.recheck`) | oneshot, timer | file at start, reload | safe: one-shot | - |
| 17 | Exception Reporter (OnFailure and daemon helper) | `exception_reporter.env` | `hams-exception-report@` (`exception_reporter.py report-unit`, also run from daemons' `ExecStopPost=`) | oneshot | `load_key()` reads the file on every report (`post_report`) | safe: per call | - |
| 18 | FCC (US Federal Communications Commission) ULS Sync (mac1) | `fcc_uls_sync_mac1.env` (on mac1: `keys/fcc_uls_sync.env`) | `fcc-uls-sync` on the mac1 VM | oneshot, hourly timer | `get_odoo_client()`, then `rotate_own_key_if_due(client)` | safe: self-rotation, see below | - |
| 19 | FCC ULS Sync (pi500-1) | `fcc_uls_sync_pi500.env` | `fcc.uls.sync` on pi500-1 | oneshot, timer | same | safe: self-rotation, see below | - |
| 20 | GDPR CSV Export Service | `gdpr_export/gdpr_export_service_internal.key` | `gdpr.csv.export` | simple, `Restart=always` | a new `get_odoo_client()` for each export request (`_consume_token`), reload on 401 | safe: per call + reload | - |
| 21 | HamCall Verify Index Sync Service | `hamcall_sync/hamcall_verify_sync_service_internal.key` | `hamcall.idx.sync` | oneshot, timer | file at start, reload | safe: one-shot | - |
| 22 | Ham Radio Logbook API Service | `logbook_api_service_internal.key` | `adif.ingress` | simple, `Restart=always` | `get_odoo_client()` per request, reload on 401 | safe: reload | - |
| 23 | Ham Radio Logbook Sync Service | `logbook_sync_service_internal.key` | `adif.processor` | simple, `Restart=always` | one `get_odoo_client()` for the consumer's life, reload on 401 | safe: reload | - |
| 24 | Inbound Mail Ingest Service | `mail_ingest/mail_ingest_service_internal.key` | `ses.inbound.mail.ingest` | oneshot, timer | file at start, reload | safe: one-shot | - |
| 25 | Ham Radio NCVEC Sync Service | `ncvec_sync/ncvec_sync_service_internal.key` | `ncvec.sync` | oneshot, timer | file at start, reload | safe: one-shot | - |
| 26 | Pager Duty - Generalized Monitor | `pager_duty.env` | `pager-os-monitor` | simple, `Restart=always`; `EnvironmentFile=` only seeds the first key | **was** the environment at start; now `OdooClient.reload_key()` re-reads the file after a 401 | **was BUG**, fixed | hams_open #1208 |
| 27 | Ham Radio Onboarding Service | `qrz_scraper/onboarding_service_internal.key` | none: `daemons/qrz_scraper` holds only a `__pycache__` | - | nothing reads it | registered, no consumer | - |
| 28 | Relay Wildcard Cert Renewal Service | `relay_cert_renew_service_internal.key` | none: the daemon was removed and its service account is archived (`action_force_provision_all` skips it) | - | nothing reads it | registered, no consumer | - |
| 29 | Repeater Re-Crawl Service | `repeater_recrawl_service_internal.key` | `club_registry_recrawl/repeater_recrawl.py` and `event_recrawl.py`: no unit in the `MANIFEST`, none on hams1 | would be oneshot | `get_odoo_client()`, reload | safe if it is ever run; no deployed consumer | - |
| 30 | S3 Storage Manager | `s3_manager.env` | none: `hams_s3` registers the key and nothing reads the file | - | nothing reads it | registered, no consumer | - |
| 31 | Ham Radio AMSAT TLE Sync Service | `satellite_sync/satellite_sync_service_internal.key` | `amsat.tle.sync`, `relay.satellites.publish` | oneshot, timer | file at start, reload | safe: one-shot | - |
| 32 | Ham Radio Space Weather Service | `space_weather/space_weather_service.key` | `noaa-swpc-sync` | simple, `Restart=always` | `get_odoo_client()` per poll, reload on 401 | safe: reload | - |

**Remote self-rotation (rows 18 and 19).** `fcc_uls_sync/main.py` calls `rotate_own_key_if_due(client, logger)` after authenticating, once a
run. When the key is more than 59 days old the daemon fetches a new key, writes it atomically to its own `ODOO_KEY_FILE`
(`write_key_file`), sets `client.login, client.key` from the file, and proves it with a second call; only then does Odoo revoke the old key.
The unit is a one-shot on an hourly timer, so the next run reads the new file; the client that rotated already holds the new key. Odoo never
writes these files (`remote_self_rotation` is set), neither the cron nor `action_force_provision_all` touches them.

**Things that are not among the 32 keys.** `hams_relay_bridge` presents `BRIDGE_API_KEY` from `bridge.env`, matched against the Odoo
parameter `ham_relay_bridge.api_key`; it is not a registry key and is not rotated. `hams_auth_gateway`, `hams_data_relay`,
`shack_console` and `hams_simulated_band` hold no Odoo API key (the gateway's only HTTP client fetches public revocation lists and CRLs). `pager_mcp_server.py`
uses `PAGER_MCP_API_KEY` from its environment, creates a client per tool call, is not registered here and is not deployed.
`zero_sudo/daemon/json_rpc_client.py` reloads on 401 and has no caller. No Odoo module other than this one reads a key file or holds a
daemon key in memory (searched: `/opt/hams/etc/keys`, `ODOO_RPC_KEY`, `Bearer`, `/json/2` across `hams_open` and `hams_com`).
`dx.firehose` has no key by design. Its `_read_qsos()` used to call `get_odoo_client()`, which exits for want of `ODOO_KEY_FILE` (it had not
met a QSO (radio contact) yet); fixed 2026-11-05 (hams_com #3109) by reading the QSOs with the PostgreSQL login it already holds (`db_app.env`), so it
holds no daemon key, needs no registry row and no `ROTATED_KEY_RELOAD_POLICY` row.

**The guard.** `ROTATED_KEY_RELOAD_POLICY` in `hams_shared/tools/infrastructure.py` lists every non-oneshot `MANIFEST` unit and MCP wrapper
script that names a file under `/opt/hams/etc/keys/`, with its mechanism (`hams_config`, `own_reload`, `per_call`, `unused`) and the source
file that does the re-read. `RotatedKeyReloadPolicyTests` in `test_infrastructure.py` fails when a new such unit has no row, when a row
names a unit that is gone, and when a key arrives through `EnvironmentFile=` (read once by systemd) without `own_reload` or `unused`.
hams_com's `daemons/test_daemon_key_reload_coverage.py` then reads the source of each `hams_config` and `per_call` row and fails if the
daemon does not call `get_odoo_client()` or reads `ODOO_RPC_KEY` itself. This form was chosen over scanning the daemons' source alone because
the `MANIFEST` is where a daemon is added to the system, so a new unit cannot be added silently, and a table needs no guess about what
a daemon's code does; the source check closes the other half. Adding a long-running daemon that holds a key means adding a row to that table
and a row to this one.

**Known limits.** (1) A daemon that *starts* between the key file being written and the transaction committing reads a key that Odoo does not
yet know and exits; `Restart=always` brings it back. At boot `hams.daemon.keys.service` runs before the daemons. (2) A rotation that rolls
back after the file was written leaves the file ahead of the database; the next rotation repairs it. (3) The guard proves the code path
exists, not that Odoo answers a revoked key with 401 for every endpoint; the full rotation drill does that on hams1.

---

## 📖 Stories & Journeys

* [Registering a New External Daemon](docs/stories/daemon_registration.md)
* [Manual Force Provisioning](docs/stories/force_provisioning.md)
* [Automated 60-Day Key Rotation](docs/stories/key_rotation.md)
* [Lifecycle of a Daemon API Key](docs/journeys/api_key_lifecycle.md)
* [Zero-Sudo Refactoring Story](docs/stories/zero_sudo_refactoring.md)

---

## 🧪 Verification
* **register_daemon_api**: The registration flow ensures that all daemon keys are properly provisioned for correct environments, verified by [@ANCHOR: COMM_test_register_daemon_api]. `test_register_daemon_api` in `tests/test_key_registry.py` calls `register_daemon` as a Daemon Key Manager and checks that it returns True, the registry row exists with the requested `env_file_path`, the key file was written under `/opt/hams/etc/keys/`, and the service account now holds the key-usage group. It is the main test of the registration flow.

* **force_provision_all**: Bootstrapping functionality is thoroughly tested to guarantee rapid restoration in an emergency, verified by [@ANCHOR: COMM_test_force_provisioning]. `test_force_provisioning` creates a registry whose key file does not exist, calls `action_force_provision_all` as a manager and requires the file to appear; a companion test makes one daemon fail and checks the others are still provisioned and the failure is reported, not raised. Together they cover the bootstrap path used at service start.

* **security_constraints**: To prevent malicious behavior, strict sandboxing and directory path validation are verified by [@ANCHOR: COMM_test_security_constraints]. `test_security_constraints` tries to register a human (non-service) account and then a path outside `/opt/hams/etc/keys/` (`/opt/jules/test.env`) as a manager, and requires a `UserError` for each. Companion tests cover the OS-group name and the key-directory rules.

* **ui_tour**: Interactive interface flows inside Odoo for key management are comprehensively verified by [@ANCHOR: COMM_test_daemon_key_manager_tour]. `test_daemon_key_manager_tour` gives the admin user the manager group and Technical Features, then runs the `daemon_key_manager_tour` browser tour from the registry action (`/odoo?debug=1&action=daemon_key_manager.action_daemon_key_registry`), exercising the registry form and its buttons. It needs a browser.

* **unauthorized_access**: Zero-sudo barriers blocking non-system service accounts from escalating permissions are verified by [@ANCHOR: COMM_test_unauthorized_access]. `test_unauthorized_access` creates a registry as a manager and then, as an ordinary internal user, calls `_rotate_key_and_write_file` and `action_force_provision_all`, requiring an `AccessError` from each, so no one without the Daemon Key Manager group can mint or write a key.

* **key_ownership**: Finally, proper multi-tenant key isolation is checked and verified by [@ANCHOR: COMM_test_key_ownership]. `test_key_ownership` registers a daemon for a fresh service account and checks that the generated API key belongs to that service account and not to the superuser, so a leaked key carries only the daemon's narrow rights.

## Function reference (ADR 0113): key lifetime and minting helpers (`models/key_registry.py`)

The constants used below: `KEY_LIFETIME_DAYS = 90` (the default key life), `MAX_KEY_LIFETIME_DAYS = 400` and `ROTATION_AGE_DAYS = 59` (a default key is due for rotation 59 days after it was issued, 31 days before it expires).

### `[@ANCHOR: daemon_key_manager:check_key_lifetime_days]` `DaemonKeyRegistry._check_key_lifetime_days()`

**Purpose.** The `@api.constrains("key_lifetime_days", "remote_self_rotation")` check that keeps the per-registry Key Lifetime field honest. Odoo runs it on create and on write of either field; nobody calls it directly.

No arguments; it checks every record in `self` and returns nothing. A record whose `key_lifetime_days` is 0 (the default) always passes. Otherwise it raises `ValidationError` when the registry is not a Remote Self-Rotation registry ("Key Lifetime can only be set on a Remote Self-Rotation registry.") or when the value is outside `KEY_LIFETIME_DAYS` to `MAX_KEY_LIFETIME_DAYS` inclusive, that is 90 to 400 days (so 89, 401 and negative values are refused). **Special concerns.** Pure validation: no writes, no SQL, no side effects; it does not touch an existing key (`action_apply_key_lifetime` does that).

**See also.** `test_key_lifetime_is_only_for_a_remote_row_and_within_range` in `tests/test_remote_self_rotation.py`.

### `[@ANCHOR: daemon_key_manager:lifetime_days]` `DaemonKeyRegistry._lifetime_days()`

**Purpose.** The number of days a key minted for this registry lives. Used when a key is minted (`_mint_key`), when the long-lived group is needed (`_ensure_usage_group`), when a key is extended (`action_apply_key_lifetime`) and to derive the rotation age (`_rotation_age_days`).

Argument: none beyond `self`, which must be exactly one record (`ensure_one`, otherwise `ValueError`). Returns `int`: `key_lifetime_days` when it is set (non-zero), else `KEY_LIFETIME_DAYS` (90). **Special concerns.** Pure read, no side effects, safe to retry. It does not validate the field; `_check_key_lifetime_days` does that at write time.

**See also.** `test_key_lifetime_is_only_for_a_remote_row_and_within_range` in `tests/test_remote_self_rotation.py`.

### `[@ANCHOR: daemon_key_manager:rotation_age_days]` `DaemonKeyRegistry._rotation_age_days()`

**Purpose.** How old, in days, a remote daemon's key may get before it is due for rotation: the key's lifetime minus 31 days (`KEY_LIFETIME_DAYS - ROTATION_AGE_DAYS`), so a daemon always has a month to rotate before expiry. `_issue_remote_rotation` (step 1 of `rotate_own_key`) compares the time since `last_rotated` with it and answers `not_due` when the key is younger.

Argument: none beyond `self`, exactly one record (`ensure_one`). Returns `int`: 59 for the default 90-day lifetime, 334 for 365 days. **Special concerns.** Pure arithmetic on `_lifetime_days()`; no side effects.

**See also.** `test_key_lifetime_is_only_for_a_remote_row_and_within_range` in `tests/test_remote_self_rotation.py`.

### `[@ANCHOR: daemon_key_manager:ensure_usage_group]` `DaemonKeyRegistry._ensure_usage_group(user)`

**Purpose.** Makes sure a daemon service account holds the groups whose API-key duration allows the keys minted for it: `group_daemon_key_usage` (90 days), and `group_daemon_key_usage_long` when the registry's lifetime is longer than 90 days. A module upgrade replaces each service account's group set with a static list that never named these groups, so without this a later rotation fails with "You cannot exceed 1.0 days". It is called at the start of every rotation (`_rotate_key_and_write_file`, `_issue_remote_rotation`) and by `action_apply_key_lifetime`, so every path heals the membership.

| Argument | Type | Meaning |
| --- | --- | --- |
| `user` | `res.users` record | The service account to put in the group(s). Borrowed; its group cache is invalidated when a group is added. |
| returns | `None` | Nothing. |

**Special concerns.** Writes by a direct `INSERT ... ON CONFLICT DO NOTHING` into `res_groups_users_rel` (through `_grant_usage_group`) because granting a group through the ORM would need the Settings administrator group, which this service account must not have (zero-sudo rule); it also clears the registry cache. Idempotent and safe to retry: a user already in the group is left alone, and a missing group record (a partly installed module) is skipped. It runs inside the caller's transaction.

**See also.** `test_action_rotate_key_self_heals_a_service_account_missing_the_usage_group` in `tests/test_key_registry.py`.

### `[@ANCHOR: daemon_key_manager:mint_key]` `DaemonKeyRegistry._mint_key(key_name)`

**Purpose.** Creates one new API key of the registry's service account. It is the single place a key is generated, shared by the local rotation (`_rotate_key_and_write_file`, which `register_daemon`, `action_rotate_key` and the nightly cron all use) and the remote self-rotation (`_issue_remote_rotation`).

| Argument | Type | Meaning |
| --- | --- | --- |
| `key_name` | `str` | The name stored on the `res.users.apikeys` row; rotation uses `<registry name>_key`, which is how older keys are found and revoked. |
| returns | `str` | The raw key. It exists only in this return value (Odoo stores a hash), so the caller must place it in the key file or the reply and never log it. |

Argument `self`: exactly one record (`ensure_one`). The key expires `_lifetime_days()` days from now (`fields.Datetime.now()` plus that many days). **Special concerns.** It generates as the service account itself (`with_user(user_id)`), because Odoo limits an API key's life by the groups of the account that owns it; the account must already be in the usage group(s) (`_ensure_usage_group`), otherwise Odoo raises `ValidationError` ("You cannot exceed ..."). It adds a row but revokes nothing: removing older keys is the caller's job. Not idempotent: each call makes another valid key.

**See also.** `test_a_year_long_lifetime_extends_the_held_key_and_mints_year_long_keys` in `tests/test_remote_self_rotation.py`.

### `[@ANCHOR: daemon_key_manager:own_key_row_id]` `DaemonKeyRegistry._own_key_row_id(raw_key)`

**Purpose.** Finds which `res.users.apikeys` row a raw key string is. The key's hash and index are not ORM fields, so the check is SQL. `rotate_own_key` uses it to confirm that the key a remote daemon presented belongs to this registry, and `_issue_remote_rotation` uses it to remember the freshly minted key as `pending_key_id`.

| Argument | Type | Meaning |
| --- | --- | --- |
| `raw_key` | `str` | The key as the daemon holds it; only its first `INDEX_SIZE` characters (the stored index) go into the query, never the whole key. |
| returns | `int` or `None` | The row id of the live key of this account named `<registry name>_key` whose hash matches `raw_key` (`KEY_CRYPT_CONTEXT.verify`), or `None` when no such unexpired key exists. |

Argument `self`: exactly one record (`ensure_one`). **Special concerns.** Read-only SQL on `res_users_apikeys`; rows with a past expiry date are ignored. The key is compared only by hash and is not logged. Safe to retry.

**See also.** `test_two_phase_rotation_revokes_nothing_until_the_new_key_is_proven` in `tests/test_remote_self_rotation.py`.
