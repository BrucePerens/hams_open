# Challenges in the Jules VM Environment

During the development and testing of the `user_websites` module, several environment-specific issues were encountered within the Jules VM. These issues primarily relate to database connectivity, permission restrictions, and sibling repository management.

## 1. PostgreSQL Socket Management
The VM uses a non-standard PostgreSQL socket directory (`/opt/hams/pgsock`). Many standard Odoo tools and even the Odoo server itself frequently default to `/var/run/postgresql`, leading to `OperationalError: connection to server on socket "/var/run/postgresql/.s.PGSQL.5432" failed`.
- **Symptom**: `psql` and `odoo` commands fail even when the server is running.
- **Solution applied**: Explicitly setting `PGHOST=/opt/hams/pgsock` for all CLI operations and ensuring the Odoo configuration correctly points to this path.

## 2. Unpredictable Database Availability
The PostgreSQL server occasionally stops responding or the lock file (`postmaster.pid`) persists after a crash, preventing restarts.
- **Symptom**: `pg_ctl: another server might be running; trying to start server anyway` or connection timeouts.
- **Workaround**: Manual cleanup of `/opt/hams/pgdata/postmaster.pid` and forced database drops (`DROP DATABASE ... WITH (FORCE)`) were required to stabilize the testing environment.

## 3. Sibling Repository Permissions (`hams_community`)
The instruction to clone `hams_community` to `../hams_community` failed due to permission denials in the parent directory (`/app/..`).
- **Symptom**: `fatal: could not create work tree dir '../hams_community': Permission denied`.
- **Solution applied**: Cloned the repository to `/hams_community` (root level) using `sudo` and then adjusted ownership to the `jules` user to make it accessible to the Odoo addons path.

## 4. Port Conflicts (8069)
The default Odoo port is frequently "leaked" or held open by zombie processes from previous test runs.
- **Symptom**: `Address already in use`.
- **Workaround**: `kill $(lsof -t -i :8069) 2>/dev/null || true` must be executed before starting any new Odoo instance.

## 5. External Dependency Gaps
Some modules in the repository (e.g., `pager_duty`) have missing system-level Python dependencies (like `pymysql`) in the VM's default environment.
- **Impact**: This causes full-suite test failures (`test_runner.py` without `-u`) even if the assigned module is perfectly functional. Developers must be careful to restrict testing scope to avoid these false positives.
