# Distributed Redis Cache Daemon

This directory contains the Distributed Cache Manager Daemon, a standalone Python asynchronous service designed to enforce cache phase coherence.

### Functions
- **PostgreSQL Listener**: Listens for `distributed_cache_invalidation` NOTIFY events from the Odoo PostgreSQL database via `asyncpg`.
- **Redis Publisher**: Validates and pushes these invalidation events to a central Redis pub/sub queue (`odoo_cache_invalidation_bus`).
- **Connection Management**: Automatically monitors and self-heals database and Redis connections during disconnects.

### File Structure
- `cache_manager.py`: The main daemon script.

### Database privilege
This daemon only issues `LISTEN distributed_cache_invalidation` and a
constant-expression `SELECT 1` health check -- it never reads or writes a
table, so it needs no privilege beyond `CONNECT` on its target database.
`hams_shared/tools/infrastructure.py`'s own `provision_environment()` now
provisions a dedicated `cache_manager_ro` role and writes
`/opt/hams/etc/keys/cache_manager_db.env` automatically on every run (see
`_provision_cache_manager_role`) -- no manual step needed for a deployment
provisioned that way. The daemon itself refuses to start
(`_require_db_credentials()`) if that file is missing and no credentials
were otherwise supplied; there is no fallback to the full-privilege `odoo`
role. For a Postgres host `infrastructure.py`'s own local-peer-auth
provisioning can't reach (a separately administered remote database), run
`../scripts/provision_cache_manager_db_role.py` directly instead -- it
writes the identical env file (loaded by `cache_manager.py` separately
from `daemon_key_manager`'s own `cache_manager.env`, which is truncated
and rewritten on every install/upgrade/key rotation).
