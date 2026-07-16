# Backup Management Daemon

This directory contains the backup worker daemon for `hams_open`. It is an asynchronous Python service that consumes `backup_tasks` from RabbitMQ. 

### Functions
- **Job Processing**: Processes incoming backup jobs and updates their state via Odoo's JSON-2 API.
- **Backup Engines**: Integrates with multiple backup engines including `kopia` and `pgbackrest` and executes backup workflows.
- **Restore Drills**: Performs restore drills securely with strict path validation and restricted binary execution.
- **Self-Healing**: Handles connection errors gracefully, throttles log updates, and ensures task consumption resilience.

### File Structure
- `main.py`: The main entrypoint for the RabbitMQ consumer.
