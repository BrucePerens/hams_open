# JULES ISSUES - backup_management

## Environment Hurdles
- **PostgreSQL Socket**: Initial tests failed because `pg_isready` and `tools/test.py` couldn't connect to the unix socket. Manually starting postgres via `sudo -u postgres /usr/lib/postgresql/18/bin/pg_ctl` with the correct config file resolved this.
- **Database Rebuild**: `createdb` sometimes fails if the database already exists and `dropdb` isn't forceful enough or is blocked by active connections.
- **UI Tour Assets**: The headless Chrome environment sometimes fails to resolve assets from other modules (like `@zero_sudo/js/tour_utils`) even when they are installed. This was worked around by inlining necessary tour utilities.
- **Test Runner Bug**: Found a bug in `tools/test.py` in `FailureExtractor.finish_and_write` (line 261) where it attempts to access a non-existent attribute `extendgrouped_blocks`.

## Framework Bugs
- **DBUS/DISPLAY**: Chrome headless requires `DBUS_SESSION_BUS_ADDRESS=autolaunch:` in this environment to start the devtools port reliably.
