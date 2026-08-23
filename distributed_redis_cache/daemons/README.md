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
Left unconfigured it authenticates as the full-privilege `odoo` role by
default. Run `../scripts/provision_cache_manager_db_role.py` once per
deployment to create a dedicated `cache_manager_ro` role scoped to
CONNECT-only and point the daemon at it (writes
`/opt/hams/etc/keys/cache_manager_db.env`, loaded by `cache_manager.py`
separately from `daemon_key_manager`'s own `cache_manager.env`, which is
truncated and rewritten on every install/upgrade/key rotation).
