# Backup Management Daemon

This directory contains the backup worker daemon for `hams_open`. It is an asynchronous Python service that consumes `backup_tasks` from RabbitMQ. 

### Functions
- **Job Processing**: Processes incoming backup jobs and updates their state via Odoo's JSON-2 API.
- **Backup Engines**: Integrates with multiple backup engines including `kopia` and `pgbackrest` and executes backup workflows. `pgbackrest backup` is not run by this daemon directly: it is handed to the privileged sidecar `pgbackrest_sidecar.py` through request and result files in `PGBACKREST_SPOOL_DIR` (see `_run_pgbackrest_via_sidecar()` in `main.py`).
- **Restore Drills**: Performs restore drills securely with strict path validation and restricted binary execution. A drill script must be an executable `.py` file under `BACKUP_WORKER_SCRIPTS_DIR` (default `/opt/hams/daemons/backup_worker/scripts`); anything else fails the job.
- **Self-Healing**: Handles connection errors gracefully, throttles log updates, and ensures task consumption resilience. Concretely: after a RabbitMQ or Odoo API error in the main loop it waits 5 or 10 seconds and reconnects; it sends log output to Odoo at most once every 2 seconds; and an error in one job marks that job `failed` and acknowledges its message, so the daemon keeps consuming. It does not repair a missing binary: if `kopia` or `pgbackrest` is not on `PATH`, the job fails.

### File Structure
- `main.py`: The main entrypoint for the RabbitMQ consumer.
- `pgbackrest_sidecar.py`: The privileged sidecar that runs `pgbackrest backup` as the `postgres` user on the daemon's behalf.
- `test_main.py`, `test_pgbackrest_sidecar.py`: Tests for the two programs above.

### Environment
- `RMQ_USER`, `RMQ_PASS`: RabbitMQ credentials. Both are required; the daemon refuses to start without them rather than fall back to `guest`/`guest`.
- `RABBITMQ_HOST`: RabbitMQ host (default `rabbitmq`).
- `ODOO_URL` (or `ODOO_HOST`), `DB_NAME`: Where the JSON-2 API calls go.
- `ODOO_RPC_KEY`: API key for the `backup_service_internal` account.
