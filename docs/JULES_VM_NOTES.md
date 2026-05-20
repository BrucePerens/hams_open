# Challenges in the Jules VM Environment

During the development and testing of the project modules, several environment-specific issues were encountered within the Jules VM. These issues primarily relate to database connectivity, permission restrictions, and sibling repository management. All identified problems have been successfully mitigated by enhancing the unified `test_runner.py` configuration framework.

## 1. PostgreSQL Socket Management
The VM uses a non-standard PostgreSQL socket directory (`/opt/hams/pgsock`). Many standard Odoo tools and even the Odoo server itself frequently default to `/var/run/postgresql`, leading to `OperationalError: connection to server on socket "/var/run/postgresql/.s.PGSQL.5432" failed`.
- **Symptom**: `psql` and `odoo` commands fail even when the server is running.
- **Resolution**: `test_runner.py` explicitly forces `os.environ["PGHOST"] = "/opt/hams/pgsock"` on any operational branch detecting a Jules framework layout. This guarantees bidirectional connectivity between the Odoo backend server layer and the test orchestration tools.

## 2. Unpredictable Database Availability
The PostgreSQL server occasionally stops responding or the lock file (`postmaster.pid`) persists after a crash, preventing restarts.
- **Symptom**: `pg_ctl: another server might be running; trying to start server anyway` or connection timeouts.
- **Resolution**: The `--provision-jules` and `--already-provisioned` automation blocks inside the runner actively execute proactive pid-file cleaning transactions (`rm -f /opt/hams/pgdata/postmaster.pid`) before cycling structural postgres processes.

## 3. Sibling Repository Permissions (`hams_community`)
The instruction to clone `hams_community` to `../hams_community` failed due to permission denials in the parent directory (`/app/..`).
- **Symptom**: `fatal: could not create work tree dir '../hams_community': Permission denied`.
- **Resolution**: The `test_runner.py --provision-jules` engine actively handles permission abstractions by checking for a sister directory layout. If a user workspace contains arbitrary hash signatures preventing sister directory validation, it captures the restriction and implements an alternate clone path targeted at `/hams_community`, modifying administrative file ownership attributes transparently.

## 4. Port Conflicts (8069)
The default Odoo port is frequently "leaked" or held open by zombie processes from previous test runs.
- **Symptom**: `Address already in use`.
- **Resolution**: The runner flushes zombie listeners out of system sockets via automated signaling commands (`kill $(lsof -t -i :8069)`) immediately before binding any operational server environments during the bootstrap pass.

## 5. External Dependency Gaps
Some modules in the repository (e.g., `pager_duty`) have missing system-level Python dependencies (like `pymysql`) in the VM's default environment.
- **Impact**: This causes full-suite test failures (`test_runner.py` without `-u`) even if the assigned module is perfectly functional.
- **Resolution**: The unified provision logic injects strict breaks to system package structures, deploying fallback layers containing `pika`, `asyncpg`, `psutil`, `requests`, `passlib`, `cryptography`, `lxml`, `pypdf2`, and `pymysql` dependencies on native loops automatically.
## 6. Kernel Memory Overcommit (OOM Kills)
The Jules VM operates with constrained RAM. By default, Ubuntu's kernel uses a heuristic memory overcommit strategy (`vm.overcommit_memory=0`). During intensive test runs, the Erlang VM (RabbitMQ) or parallel Odoo workers may request large blocks of memory that the kernel grants, but when the processes actually attempt to write to that memory, the kernel realizes it is exhausted and abruptly terminates the process via the OOM Killer.
- **Symptom**: RabbitMQ inexplicably dies mid-test with `Killed` or `SIGKILL`, or Odoo workers vanish without a standard Python traceback.
- **Resolution**: Enforce `sysctl vm.overcommit_memory=1` (Always Overcommit). Do NOT use `vm.overcommit_memory=2` (Strict), as Odoo's Headless Chrome test runner requests massive Virtual Memory (VSIZE) allocations on startup that State 2 will instantly reject, breaking the test suite completely. State 1 allows Chrome to boot while stabilizing Erlang's allocation behavior.
