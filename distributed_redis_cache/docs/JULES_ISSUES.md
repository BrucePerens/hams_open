# JULES ISSUES (Distributed Redis Cache)

## Environment Issues
- **PostgreSQL Socket Permissions:** The local PostgreSQL socket at `/opt/hams/pgsock` was initially restricted, causing `psycopg2.OperationalError: Permission denied` when Odoo (running as `odoo` user) attempted to connect. Resolved by `chmod 777 /opt/hams/pgsock`.
- **Chrome Headless in Jules VM:** Chrome failed to detect the devtools port, causing UI tours to be skipped in standard mode.
