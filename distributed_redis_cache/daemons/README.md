# Distributed Redis Cache Daemon

This directory contains the Distributed Cache Manager Daemon, a standalone Python asynchronous service designed to enforce cache phase coherence.

### Functions
- **PostgreSQL Listener**: Listens for `distributed_cache_invalidation` NOTIFY events from the Odoo PostgreSQL database via `asyncpg`.
- **Redis Publisher**: Validates and pushes these invalidation events to a central Redis pub/sub queue (`odoo_cache_invalidation_bus`).
- **Connection Management**: Automatically monitors and self-heals database and Redis connections during disconnects.

### File Structure
- `cache_manager.py`: The main daemon script.
