# Distributed Redis Cache Daemon

This directory contains the Distributed Cache Manager Daemon, a standalone Python asynchronous service designed to enforce cache phase coherence: once any Odoo worker signals that cached data changed, every other worker discards its in-memory cache before its next request or cron job (see `../README.md`, Architecture).

### Functions
- **PostgreSQL Listener**: Listens for `distributed_cache_invalidation` NOTIFY events (PostgreSQL's built-in message broadcast) from the Odoo PostgreSQL database via `asyncpg`.
- **Redis Publisher**: Validates each payload (it must be a JSON object with both `model` and `dbname`; anything else is logged and dropped), then, in one Redis pipeline, publishes it on the pub/sub channel `odoo_cache_invalidation_bus` and increments the `global_cache_invalidation_counter` key. Odoo workers read only the counter; nothing in Odoo subscribes to the channel, which this module's tests use to verify the publish side.
- **Connection Management**: For PostgreSQL, it runs a `SELECT 1` health check every 60 seconds (10-second timeout) and, on any failure, closes the connection and reconnects 5 seconds later; a failed connection attempt is logged and retried on the next 60-second cycle. For Redis there is no reconnect loop: if Redis is unreachable at startup the daemon exits (the provided systemd unit, `Restart=always`, starts it again 10 seconds later), and after startup a publish that fails or times out (10 seconds) is logged and that one invalidation is dropped, not retried.

### File Structure
- `cache_manager.py`: The main daemon script.
- `cache-manager.service`: systemd unit that runs it as the `odoo` user, with Redis credentials from `/opt/hams/etc/redis.env`. hams_com's provisioning MANIFEST carries the identical text (a test there fails if they drift).
- `test_cache_manager.py`: unit tests for the daemon.

### Database privilege
This daemon only issues `LISTEN distributed_cache_invalidation` and a
constant-expression `SELECT 1` health check -- it never reads or writes a
table, so it needs no privilege beyond `CONNECT` on its target database.
`hams_shared/tools/infrastructure.py`'s own `provision_environment()` now
provisions a dedicated `cache_manager_ro` role and writes
`/opt/hams/etc/keys/cache_manager/cache_manager_db.env` automatically on every run (see
`_provision_cache_manager_role`) -- no manual step needed for a deployment
provisioned that way. A failure in that step is logged as a warning and
does not stop provisioning, so if the daemon will not start, check that
the file exists. The daemon itself refuses to start
(`_require_db_credentials()`) if that file is missing and no credentials
were otherwise supplied; there is no fallback to the full-privilege `odoo`
role. For a Postgres host `infrastructure.py`'s own local-peer-auth
provisioning can't reach (a separately administered remote database), run
`../scripts/provision_cache_manager_db_role.py` directly instead -- it
writes the identical env file, which you then hand to the daemon's account
(`chown odoo:hamsd_cache_manager`, `chmod 0640`).

### Account and files
The daemon runs as its own system account, `hamsd_cache_manager` (hams.com's daemon OS isolation, one account per
daemon family), not as `odoo`. The daemon reads its login only from the family key directory's
`keys/cache_manager/cache_manager_db.env` (`DB_ENV_FILE`, [@ANCHOR: COMM_cache_manager_config]; `odoo:hamsd_cache_manager`,
`0640`, in a `0750` directory of the same owner and group). There is no fallback to the old flat
`keys/cache_manager_db.env` (deleted from `hams1` on 2026-11-07): without the new file the daemon logs that it is not
found and refuses to start (`_require_db_credentials()`). The daemon no longer opens `daemon_key_manager`'s `keys/cache_manager.env`: it never used
the Odoo login in it, and that `odoo`-only file is unreadable to its own account. Run it by hand as
`sudo -u hamsd_cache_manager python3 cache_manager.py`, not as `odoo`.
