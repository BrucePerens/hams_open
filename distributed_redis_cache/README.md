# Distributed Redis Cache (`distributed_redis_cache`)

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).*

The Distributed Redis Cache module replaces Odoo's per-process method cache (`@tools.ormcache`) with a high-performance, distributed Redis backend: methods decorated with `@distributed_cache()` (see the Developer & AI Usage Guide below) share their cached results through Redis instead of each worker computing and holding its own copy. Odoo's own ORM record cache is not modified. This enables true horizontal scaling for Odoo Community by enforcing phase coherence across multiple WSGI workers and completely separate physical web servers. "Phase coherence" here means: once any worker signals that cached data changed, every other worker discards its in-memory copies before it serves its next HTTP request or runs its next cron job, so workers do not drift onto different versions of the same cached data (see Architecture below).

## 🛠️ Dependencies & Installation

Because this is a standalone Open Source module that relies on an external Python daemon (`daemons/cache_manager.py`, which you run as its own systemd service; the module does not start it), you must ensure its system dependencies are satisfied before installing it in your database. The module will fail-fast at startup if these are missing.

**External Dependencies:** `python-dotenv`

**Required Python Modules:** `redis`, `asyncpg`, `python-dotenv`

* **Debian/Ubuntu Installation:**
    ```bash
    sudo apt-get install python3-redis python3-asyncpg python3-dotenv
    ```
* **Pip Installation:**
    ```bash
    pip3 install redis asyncpg python-dotenv
    ```

**System Requirements:**
* A running `redis-server` instance.

## 🌟 Key Features

* **Phase Coherence:** Uses PostgreSQL `NOTIFY` and a lightweight Python daemon (`cache_manager.py`) to increment a shared Redis counter that every worker checks before each HTTP request and each cron job (see Architecture).
* **Granular Invalidation:** Instead of Odoo's process-wide `clear_caches()`, invalidating a model deletes only that model's entries for that database from Redis (keys beginning `<dbname>:distributed_cache:<model>:`). Other workers still clear their whole in-memory cache when they see the signal (see Architecture step 4).
* **Control Panel:** Provides a backend form, the Distributed Cache Manager, to check that this Odoo server can reach Redis (**Check Redis Status**) and to invalidate one model's cache on demand (**Invalidate Model Cache**).
* **Zero-Sudo Security:** "Zero-Sudo" is this project's rule against Odoo's `.sudo()` superuser mode, enforced by the `zero_sudo` module this one depends on. The cache-manager daemon goes further and needs no Odoo login at all: it runs as its own unprivileged OS account, `hamsd_cache_manager`, with `NoNewPrivileges=true` (see `daemons/cache-manager.service`) and connects to PostgreSQL as a dedicated role that needs only `CONNECT` (see `daemons/README.md`). [@ANCHOR: COMM_story_zero_sudo_cache_manager]

## 🚀 Getting Started

1. Ensure the Python dependencies (`redis`, `asyncpg`) are installed on your host.
2. Install the `distributed_redis_cache` module from the Odoo Apps menu.
3. Configure Redis settings:
   * Navigate to **Settings > Distributed Redis Cache** (its own section of the main Settings page; the Distributed Cache Manager form has no connection fields). [@ANCHOR: COMM_distributed_cache_settings_view]
   * Enter your Redis host, port, and password. These are stored as system parameters (`ir.config_parameter` keys `distributed_redis_cache.redis_host`, `distributed_redis_cache.redis_port` and `distributed_redis_cache.redis_password`; the password field is masked in the form). Until they are set, Odoo uses the `REDIS_HOST`, `REDIS_PORT` and `REDIS_PASSWORD` environment variables, defaulting to host `redis`, port 6379, no password. Saving the settings makes every worker reconnect with the new values: the saving worker immediately, every other worker at its next request or cron job via the same invalidation signal described under Architecture.
4. Start the background sync daemon:
   * Execute `daemons/cache_manager.py` as a systemd service (a unit file is provided at `daemons/cache-manager.service`; it loads `/opt/hams/etc/redis.env`, so a Redis that requires a login works). On hams.com, hams_com's `infrastructure.py` MANIFEST installs the identical unit as `cache-manager.service`, and `devbox_tools/production_readiness_check.py` fails when the unit is not running or the counter key does not exist. Without this daemon the `global_cache_invalidation_counter` key never exists and no worker ever sees another worker's invalidation.
   * The daemon requires `/opt/hams/etc/keys/cache_manager/cache_manager_db.env` (readable by its account, `hamsd_cache_manager`), which holds `DB_USER`/`DB_PASS` for its restricted PostgreSQL role; it refuses to start without those credentials. See `daemons/README.md` for how that file is provisioned.
   * It does not read `/opt/hams/etc/keys/cache_manager.env`, the Daemon Key Manager's Odoo login for this service: the daemon has no use for an Odoo login. Since version 1.1 the module no longer registers that key at all (see `unregister_cache_manager_key()` below), so a new install never creates the file.
   * The daemon does **not** read the Redis settings saved in Odoo. It takes `REDIS_HOST`, `REDIS_PORT` and `REDIS_PASSWORD` from its own environment, defaulting to `localhost`, 6379 and no password (note that the Odoo side defaults to host `redis`), and `DB_HOST`, `DB_PORT` and `DB_NAME` likewise, defaulting to `localhost`, 5432 and `odoo`. It listens on exactly one database, `DB_NAME`. Set these for the service when your deployment differs from the defaults.
5. Navigate to **Settings > Technical > Distributed Cache** in Odoo to verify the connection status. The menu is shown to members of the Cache Manager group, which includes the administrator by default. The status check tests only this Odoo server's own Redis connection, not the daemon. [@ANCHOR: COMM_distributed_cache_view]

## Upgrading modules on a server (Redis credentials)

Production Redis refuses unauthenticated clients. The odoo systemd service gets its credentials from
`EnvironmentFile=` entries (`/opt/hams/etc/redis.env` among them); a hand-run `sudo -u odoo odoo -u ...` does **not**,
and every cache invalidation of such an upgrade fails ("Authentication required") while the upgrade itself succeeds,
leaving the whole Redis L2 cache stale for up to 24 hours (found 2026-10-12, 2394 failed invalidations).

Run upgrades with the service's environment:

```
sudo pagerduty-maintenance start --minutes 30 --reason "odoo upgrade"
sudo systemctl stop odoo
sudo python3 /opt/hams/src/hams_open/distributed_redis_cache/scripts/odoo_upgrade.py --db hams_prod --modules mod_a,mod_b --log /tmp/upgrade.log   # exit 4: do NOT start odoo
sudo systemctl start odoo
sudo pagerduty-maintenance end
```

A one-off `odoo shell` that writes (a runbook step that calls an ORM method, then commits) needs the same environment, or each write logs a failed Redis cache invalidation: `sudo python3 .../odoo_upgrade.py --db hams_prod --shell < script.py` runs it in the unit `odoo-shell-run` with the service's environment files (the service may keep running; the script's output and the exit code come straight through; `--dry-run` prints the command).

`odoo_upgrade.py` reads the service's own `EnvironmentFiles=`/`Environment=`/`User=` from `systemctl show` and runs
`odoo -u` in the transient `systemd-run` unit `odoo-upgrade` (check `systemctl is-active odoo-upgrade`; do not `systemctl restart odoo` or copy a deploy while it is active: it holds the HTTP port, so a restart fails "Address already in use" until it ends, as on 2026-10-12) with them (values are never read or printed), refuses to run if the service
defines no `REDIS_PASSWORD` or is still running, and exits non-zero if the output shows any Redis authentication failure.
Independently, `invalidate_model_cache` raises instead of warning when Redis rejects the credentials while the registry
is still loading (module install/upgrade; [@ANCHOR: distributed_redis_cache:COMM_invalidation_auth_failure_aborts_upgrade]),
so a hand-run upgrade fails at its first cache invalidation instead of logging thousands of warnings. After a successful upgrade, and before odoo is started, it
compiles every stylesheet bundle (`scripts/check_asset_bundles_shell.py` piped into `odoo shell --no-http` in the unit `odoo-upgrade-css-check`, same environment, nothing written) and
exits 4, printing the compiler's message and the rollback steps, if one fails: Odoo otherwise serves HTTP 200 with a "css error" stub stylesheet and the whole site is unstyled (hams.com, 2026-10-14). A run without a verdict line counts as a failure;
`--install a,b` adds `-i a,b` to the same run (argument: comma-separated module names, each `[a-z0-9_]+`; refused otherwise, exit 2) for a module that is new on the database: `-u all` leaves a new `auto_install` module `uninstalled` (the 2026-11-08 rehearsal on a restored `hams_prod` saw `ham_spreadsheet_csp` stay uninstalled with its dependencies installed). `--no-css-check` skips it. A serving worker
still only logs a warning and falls back to its local cache. If an upgrade did run unauthenticated: delete the `<db>:distributed_cache:*` keys with `SCAN`+`DEL` (never
`FLUSHALL`) and restart odoo. A restart alone does not help: it clears only each worker's in-memory L1, not Redis.

## 🏗️ Architecture (CQRS)

This module implements a strict CQRS (Command Query Responsibility Segregation) pattern to bypass Odoo's single-threaded limitations:
1. When code that changed cached data calls `notify_model_invalidation(env, model_name)` (or an administrator saves the Redis settings, or uses **Invalidate Model Cache**), the worker runs `SELECT pg_notify('distributed_cache_invalidation', <payload>)`, where the payload is the JSON object `{"model": ..., "dbname": ...}`. Nothing is sent automatically when records are written, views change or modules are installed: each module whose cached results depend on some data must call `notify_model_invalidation()` itself when that data changes. PostgreSQL delivers the notification only when the transaction commits. After the same commit, a `notify_model_invalidation()` caller (including the **Invalidate Model Cache** button) also deletes that model's Redis entries and its own in-memory entries for that model (see `invalidate_model_cache` below); the Settings save clears only the saving worker's cached connection settings.
2. The standalone `cache_manager.py` daemon, utilizing `asyncpg`, instantly catches the PostgreSQL notification. It ignores (and logs) any payload that is not a JSON object with both `model` and `dbname`.
3. The daemon publishes the invalidation payload to a Redis Pub/Sub bus AND increments a single global Redis counter key, in the same pipeline.
4. **Corrected 2026-09-09**: no Odoo WSGI worker actually subscribes to that Pub/Sub bus (it exists only so the daemon's own test suite can verify the publish side against a real subscriber) -- there is no background listener thread anywhere in this module. What every worker actually does is poll the global counter with a synchronous Redis `GET` at the start of each HTTP request (`ir.http._authenticate`) and before each cron job (`ir.cron._process_job`, since cron dispatch never passes through `_authenticate`); if it changed since the last request this worker handled, the worker immediately flushes its *entire* local in-memory LRU (least-recently-used, capped at 8192 entries) cache (every model, not only the one that changed), and its cached Redis connection settings, before continuing. The poll is skipped only while the registry is still loading (module install or upgrade), so an unreachable Redis cannot fail an upgrade. While the daemon runs and Redis is reachable, this still guarantees no worker ever serves a value from before the last invalidation -- it is just a request-time poll-and-clear-all, not a push-based, per-model subscription. If the daemon is not running, the counter never changes; if Redis is unreachable, the poll fails with a logged warning. In either case other workers keep serving their in-memory entries, which have no expiry of their own, until a later counter change or a process restart.

## 💻 Developer & AI Usage Guide

This module provides essential tools to integrate customized models into the distributed caching ecosystem safely. When developing new modules, always use the functions detailed below instead of standard Odoo caching (`@tools.ormcache`).

### 1. The `@distributed_cache()` Decorator [@ANCHOR: COMM_distributed_cache_decorator]
Replaces Odoo's `@tools.ormcache` to provide precise, cross-worker distributed caching with multi-tenant awareness (website and company boundaries).

**Purpose.** Caches the result of an Odoo model method in two levels: a per-process in-memory cache (L1) and the shared Redis store (L2), keyed so that tenants never see each other's results. It replaces `@tools.ormcache`, which only knows about one worker.

**Call interface.** `distributed_cache()` takes no arguments and returns the decorator, so it is always written with parentheses. The decorated function must be a method of an Odoo model (the wrapper reads `self.env` and `self._name`); it may take any positional and keyword arguments and keeps its own name and docstring. The wrapper does this, in order:

| Step | What happens |
|---|---|
| Bypass | If `redis_bypass_cache` is truthy in the context, the method is called directly and nothing is read or written. |
| Key | `<dbname>:distributed_cache:<model>:<method>:w<website_id>:c<company ids>:<argument hash>`. `website_id` is the context's `website_id` or 0; the company part is the sorted `allowed_company_ids` from the context, else the sorted ids of `env.companies`, else `0`; the hash is `_get_hash` (see `COMM_distributed_cache_key_generation`) and includes `self`. |
| L1 | The process-wide `_local_cache` (LRU, 8192 entries, guarded by `LRU_LOCK`) is checked first; a hit returns the stored object itself, not a copy. |
| L2 | With an HMAC (keyed-hash message authentication code) key (`_cache_hmac_key` is not `None`) and a Redis pool, the key is read, the HMAC verified, the payload unpickled and copied into L1. |
| Miss | The method runs; the result is pickled, signed and written with `SETEX` for 86400 seconds (24 hours), and is always stored in L1. |

**Returns and errors.** The method's own return value. An exception raised by the method propagates and nothing is cached. A `redis.RedisError` is logged as a warning and the call carries on without Redis (fail open). A payload that is malformed, fails the HMAC check or will not unpickle is logged and ignored, and Redis is not written for that call. A result that cannot be pickled is returned and kept in L1 only.

**Special concerns.** Synchronous and blocking: a Redis call waits at most the pool's 1 second socket timeout. The only lock taken is `LRU_LOCK`, briefly and never across a Redis call or the method itself. Callers share the cached object, so they must not mutate a returned list or dict, and a method whose result depends on something that is not in the key (the user, the language, the time) must not be decorated. L1 entries have no expiry of their own: they go when `poll_and_clear_local_cache` sees the invalidation counter change, when `invalidate_model_cache` runs for the model, or when the process restarts. Redis entries live 24 hours. Pickle is only read back after the HMAC verified, so a writer without the application secret cannot plant code.

**See also.** Test: `tests/test_distributed_cache.py` (cites this anchor); related `COMM_notify_model_invalidation_logic`, `COMM_invalidate_model_cache_logic`, `distributed_redis_cache:COMM_get_redis_connection`, `distributed_redis_cache:COMM_cache_hmac_key`.

**Usage:**
```python
from odoo.addons.distributed_redis_cache.redis_cache import distributed_cache
from odoo import models

class MyModel(models.Model):
    _name = 'my.model'

    @distributed_cache()
    def my_expensive_computation(self, param1):
        # Result will be cached in Redis and local memory across all workers
        return ...
```

**How it caches:** the cache key combines the database name, model name, method name, `website_id` and the active company IDs (from `allowed_company_ids` in the context, or else the user's companies), and a SHA-256 hash of the recordset IDs and arguments. A call first checks the worker's in-memory cache, then Redis; on a miss it runs the method and stores the result in both, with a 24-hour expiry in Redis. Results must be picklable (serializable with Python's `pickle`); one that is not is still returned and kept in memory, but is not written to Redis. Redis payloads are HMAC-signed (a keyed SHA-256 signature) and any payload whose signature does not verify is discarded, never unpickled. The signing key is derived from `HAMS_CRYPTO_KEY`, else `/var/lib/odoo/hams_crypto.secret`, else Odoo's `admin_passwd` (the literal `admin` is refused). With none of these configured, the decorator skips Redis entirely and caches per process only. If Redis is unreachable, the method is computed and cached in memory only ("fail-open"), so an outage slows the site down but does not break it. Passing `redis_bypass_cache=True` in the context skips caching entirely.

### 2. `notify_model_invalidation(env, model_name)`
Triggers a cross-worker invalidation signal via PostgreSQL `NOTIFY`. Use this when you update records that are aggressively cached and need immediate propagation. Nothing happens until the current transaction commits: then this worker clears the model's Redis and in-memory entries, and the notification reaches the daemon, which signals every other worker as described under Architecture. A `model_name` that is not a model in this database is refused with a logged warning and no signal. [@ANCHOR: COMM_notify_model_invalidation_logic]

**Usage:**
```python
from odoo.addons.distributed_redis_cache.redis_cache import notify_model_invalidation

def invalidate_my_model(self):
    # Sends a PostgreSQL (PG) NOTIFY to invalidate 'my.model' caches globally
    notify_model_invalidation(self.env, 'my.model')
```

### 3. `invalidate_model_cache(env, model_name, local_only=False)`
Flushes the cache specifically for the given model locally, and also within the Redis store unless `local_only=True`. `notify_model_invalidation()` calls it for you after commit; daemon events do not call it (other workers react to them by clearing their whole in-memory cache instead). Calling it directly sends no signal: other workers keep their in-memory copies until some later invalidation changes the counter. Use `notify_model_invalidation()` when other workers must see the change. [@ANCHOR: COMM_manual_cache_invalidation]

**Usage:**
```python
from odoo.addons.distributed_redis_cache.redis_cache import invalidate_model_cache

def force_clear(self):
    # Clears local and Redis cache synchronously for 'my.model'
    invalidate_model_cache(self.env, 'my.model')
```

**Testing Note:** The module disables test bypass for caching to ensure parity with production: tests use the same Redis and counter-polling paths as a serving worker. If you encounter test pollution, pass the `.with_context(redis_bypass_cache=True)` context variable during your environment operations; the `@distributed_cache()` wrapper then calls the method directly, neither reading nor writing any cache. The Redis Cache Interceptor (the `ir.http._authenticate` override that runs the per-request counter poll described under Architecture) is separate from this flag and does not consult it. [@ANCHOR: COMM_redis_cache_interceptor]

### 5. Internals worth knowing

* `poll_and_clear_local_cache(env)` [@ANCHOR: distributed_redis_cache:COMM_poll_and_clear_local_cache] reads Redis's `global_cache_invalidation_counter`; if it changed since the last poll by any caller in this process, it empties the in-memory cache and this database's cached Redis connection settings. `ir.http._authenticate` and `ir.cron._process_job` both call it, so a worker notices another worker's change on its next request or cron run. A Redis error is logged as a warning and nothing is cleared.
* `clear_db_config_cache(dbname)` [@ANCHOR: distributed_redis_cache:clear_db_config_cache] forgets the cached (host, port, password) of one database, so the next `get_redis_connection(env)` reads the current Settings values. It runs when Redis settings are saved (on the saving worker) and from `poll_and_clear_local_cache` (on every other worker).
* `res.config.settings.get_values()` [@ANCHOR: distributed_redis_cache:res_config_settings_get_values_masks_password] is `@api.private` and returns a mask in place of the stored Redis password, so neither the settings form nor a `default_get` reveals it; saving the mask unchanged keeps the stored password.
* `res.config.settings.set_values()` [@ANCHOR: distributed_redis_cache:res_config_settings_set_values] saves the Redis settings, clears the saving worker's cached connection settings, and sends the `distributed_cache_invalidation` `pg_notify` so the daemon bumps the global counter and every other worker clears theirs too; without that, a saved change would reach each worker only when it was restarted.
* The cache-manager daemon's `_require_db_credentials()` [@ANCHOR: COMM_cache_manager_require_db_credentials] raises `RuntimeError` at start-up unless both `DB_USER` and `DB_PASS` are set, so it never connects to PostgreSQL without its own scoped role.
* The daemon applies that login file with `_apply_db_env_file()` [@ANCHOR: COMM_cache_manager_db_env_precedence]: the file's `DB_USER`/`DB_PASS` always replace anything already exported, but its `DB_HOST`/`DB_PORT`/`DB_NAME` are only defaults, so a caller that exports `DB_NAME` (a test run serving its own slot database) is not overridden.

### 6. `expiring_counter(client, key, window_seconds, amount=1)`: a rate-limit counter that always expires [@ANCHOR: distributed_redis_cache:expiring_counter]

`from odoo.addons.distributed_redis_cache.redis_pool import expiring_counter, redis_pool`

Use it for every fixed-window counter that refuses a member or an address after N hits per window (rate limits, per-hour caps).
The pattern it replaces, `count = client.incr(key)` followed by `if count == 1: client.expire(key, window)`, is two commands: a worker
killed between them, or an `expire` that raises `RedisError`, left the key with no time to live. The count then only grew, passed the
limit, and the member or address was refused for ever until someone deleted the key by hand.

One `MULTI`/`EXEC` transaction now runs `SET key 0 NX EX window`, `INCRBY key amount` and `TTL key`, so a counter is never created without its
expiry. A key an older caller already left without one (TTL -1) is given the window on its next use (one extra `EXPIRE`), so such keys heal
themselves.

| Argument | Type and range | Meaning |
|---|---|---|
| `client` | `redis.Redis` | The connection, normally `redis.Redis(connection_pool=redis_pool)`. Must support `pipeline(transaction=True)` and `expire`. |
| `key` | `str` | The counter's name. The caller includes the database name, for example `f"{dbname}:ratelimit:thing:{who}"`. |
| `window_seconds` | `int`, at least 1 | Lifetime of a counter that starts with this hit. The window is fixed: it starts at the first hit; later hits neither extend nor reset it. |
| `amount` | `int`, 0 or more, default 1 | What this hit adds (`INCRBY`). A caller that weighs its hits passes it, for example the console's callbook search adds the number of contact rows it returned; a rate limit that counts requests leaves it at 1. |

| Result | Meaning |
|---|---|
| `int` | The count including this hit's `amount`; `amount` (1 by default) for the first hit of a window. The caller compares it with its own limit, and may take its own amount back with `DECRBY` when it is over. |
| `redis.RedisError` (raised) | Any Redis failure, connection errors included, passes through unchanged. The caller chooses to fail open (abuse mitigation where other limits bound the damage) or closed (nothing else bounds the work). The helper makes no choice for it. |

Behaviour worth knowing: every call counts (not idempotent); the transaction is one round trip, and the only blocking is the client's own
socket timeout; there are no threads or locks in the helper; a crash after the `SET` but before the `INCR` leaves a count of 0 that
expires on schedule. Tests: `tests/test_expiring_counter.py` (counting and a window the later hits do not extend, `redis.Redis.expire`
patched to always raise, a legacy key without a TTL, an error reaching the caller, an `amount` added in one step). A caller's test makes the same failure with
`self.safe_patch_object(redis.Redis, "expire", side_effect=redis.RedisError("..."))`.

### 7. `unregister_cache_manager_key(env)`: drop the unused Daemon Key Manager registration [@ANCHOR: distributed_redis_cache:unregister_cache_manager_key]

`from odoo.addons.distributed_redis_cache.hooks import unregister_cache_manager_key`

Versions before 1.1 registered the daemon "Redis Cache Manager" with the Daemon Key Manager in a `post_init_hook`, so the registry minted an
Odoo API key for the service account `distributed_redis_cache.cache_manager_service_internal`, rotated it every 59 days and wrote it to
`/opt/hams/etc/keys/cache_manager.env`. The cache-manager daemon has no Odoo client and never read that key (its only credential is the
CONNECT-only PostgreSQL login in `keys/cache_manager/cache_manager_db.env`), so the key was a live credential with no user. Version 1.1
removes the hook (the manifest has no `post_init_hook`) and `migrations/1.1/post-unregister-cache-manager-key.py` calls this function once
on an installed database (its `migrate(cr, version)` does nothing for a fresh install, which has no version, and raises `ValueError` when the Daemon Key Manager service account is missing) `distributed_redis_cache:migrate`.

| Argument | Type and range | Meaning |
|---|---|---|
| `env` | `odoo.api.Environment` | Any environment of the database. The function switches to the Daemon Key Manager service account (`daemon_key_manager.user_daemon_key_manager_service`) itself; nothing runs as the superuser. |

| Result | Meaning |
|---|---|
| `int` | The number of registry rows removed: 1 when the row "Redis Cache Manager" existed, 0 when it did not. |
| `odoo.exceptions.AccessError` (raised) | The Daemon Key Manager service account does not exist or may not be used; the upgrade stops. |

Behaviour worth knowing: it calls `daemon.key.registry.unregister_daemon` (daemon_key_manager's README) as the Daemon Key Manager service account, which deletes the registry row and the API keys named `Redis Cache Manager_key` of that row's service account, in the
caller's transaction (nothing commits). Idempotent, so it is safe to run again; a database that never had the row returns 0. It does not
delete the service account (it stays, with no key and no group beyond its own) and does not delete `keys/cache_manager.env` from disk: the
registry never removes a key file, so an operator deletes the leftover file by hand. It is not a security gate and takes no lock. Tests:
`tests/test_hooks.py` (a row registered exactly as version 1.0 did is removed with its key, the second call removes nothing, and the manifest
declares no `post_init_hook`).

### 7a. `migrate(cr, version)` (`migrations/1.1/post-unregister-cache-manager-key.py`): the 1.1 upgrade step [@ANCHOR: distributed_redis_cache:migrate]

**Purpose.** Upgrade step of version 1.1: removes the Daemon Key Manager registration "Redis Cache Manager" that earlier versions created, because the cache-manager daemon never used the Odoo API key it produced.

**Call interface.** `migrate(cr, version)` in `migrations/1.1/post-unregister-cache-manager-key.py`, called by Odoo after the module's 1.1 data is loaded. `cr` is the upgrade cursor; `version` the previously installed version, empty on a fresh install. Returns `None`. With no `version` it does nothing. Otherwise it looks up `KEY_MANAGER_XMLID` (`daemon_key_manager.user_daemon_key_manager_service`) in `ir_model_data`, raises `ValueError` if that account is missing (the upgrade stops), and calls `unregister_cache_manager_key` with an environment for that user.

**Special concerns.** Runs once in the upgrade's transaction (nothing commits here) and is idempotent through the function it calls. It runs as the Daemon Key Manager service account, not as superuser. It does not delete the service account or the key file on disk; an operator removes `keys/cache_manager.env` by hand.

**See also.** Test `tests/test_hooks.py`; `distributed_redis_cache:unregister_cache_manager_key`.

### 7b. `provision(admin_host, admin_port, admin_user, admin_password, target_db, role_name)` (`scripts/provision_cache_manager_db_role.py`): create the cache manager's database role [@ANCHOR: distributed_redis_cache:provision]

**Purpose.** Creates (or re-keys) the restricted PostgreSQL role the cache-manager daemon logs in with, and returns the new password, for a database host the module's own local provisioning cannot reach.

**Call interface.** Connects to the `postgres` database with the admin credentials (autocommit, closed on every path). It draws a 32-byte URL-safe random password. If `role_name` exists it runs `ALTER ROLE ... LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION PASSWORD`, so the old password stops working; otherwise `CREATE ROLE` with the same flags. It then grants `CONNECT` on `target_db` to the role and nothing else. Role and database names go through `sql.Identifier`, the password through a bound parameter. Returns the password string; it prints only the role name, never the password. A database error propagates.

**Special concerns.** Re-running rotates the password, so a running daemon fails to log in until it reads the new file. A role of the same name that was made for another purpose loses its privilege flags and its password. The role gets no table grants, but PostgreSQL's default privileges for `PUBLIC` (for example `USAGE` on schema `public`) still apply. The admin password is passed in process and never written.

**See also.** Tests `ProvisionTests` in `scripts/test_provision_cache_manager_db_role.py`; `distributed_redis_cache:write_env_file`.

### 7c. `write_env_file(path, role_name, password, admin_host, admin_port, target_db)` (`scripts/provision_cache_manager_db_role.py`): write the daemon's credentials file [@ANCHOR: distributed_redis_cache:write_env_file]

**Purpose.** Stores the role and password `provision` returned in the env file the cache-manager daemon reads, so a new or changed password never sits in a file with looser permissions.

**Call interface.** Creates the directory of `path` with mode 0700 if it is missing. Writes `DB_HOST`, `DB_PORT`, `DB_NAME`, `DB_USER` and `DB_PASS` (with two comment lines) to a new file `<path>.tmp.<pid>` created with `O_EXCL` at mode 0600, copies the owner and group of an existing file at `path` onto it (when not root, a `PermissionError` is reported and the owner stays the current user), then `os.replace`s it over `path`; the temporary file is removed when anything fails before the replace. Returns `None`. It prints a hint to give the file to the daemon's account (`chown odoo:hamsd_cache_manager`, `chmod 0640`).

**Special concerns.** The mode of an older file is never reused: an existing 0644 file ends up as a 0600 file holding the new password. The temporary name is predictable but opened with `O_EXCL`, so an existing path of that name (including a symlink) makes the call fail instead of being followed. The final mode for the daemon is set by the operator's `chmod`, not here.

**See also.** `WriteEnvFileTests` in `scripts/test_provision_cache_manager_db_role.py`; `distributed_redis_cache:provision`.

### 8. `main()` (`daemons/cache_manager.py`): the cache manager daemon loop `[@ANCHOR: distributed_redis_cache:cache_manager_main]`

**Purpose.** The whole run of `hams-cache-manager`: it listens on one PostgreSQL channel (`PG_CHANNEL`) of one database and re-publishes every notification to Redis, so every Odoo worker and server sees a cache invalidation. It is the body of `asyncio.run(main())` at the foot of the file.

No arguments, no return value; it is a coroutine (`async def`) that normally runs until cancelled. Steps and results:

| Step | Result |
| --- | --- |
| `_require_db_credentials()` | `RuntimeError` is raised before any connection is made when the database user or password is empty; the daemon never starts without a login. |
| Redis connect and `ping()` | A `redis.asyncio` client with 10 second connect and read timeouts. A `RedisError` is logged as critical and `main()` returns and the process ends. |
| LISTEN | One `asyncpg` connection to `DB_NAME` (10 second timeout) with `postgres_notify_handler` as the listener. A failure to connect is logged and retried on the next loop pass. |
| Loop | Every 60 seconds a `SELECT 1` (10 second timeout) proves the connection is alive; a closed connection is reconnected; any other error is logged, waits 5 seconds, closes the connections and reconnects. |
| `asyncio.CancelledError` | Logged as a clean shutdown; the loop ends and the Redis client is closed with `aclose()`. |

**Special concerns.** Holds the module-level `redis_client` global. Exactly one database is listened to; it does not enumerate the cluster's databases. Each notification becomes its own background task in `postgres_notify_handler`, so a stalled Redis cannot block the loop (the socket timeouts make a stall a per-call failure). The password is read only from the family key directory and is never logged.

**See also.** `test_main_refuses_to_start_without_a_database_password` in `daemons/test_cache_manager.py` (the guard is wired into `main`).

## The supervised upgrade tool (`scripts/odoo_upgrade.py`)

The anchor scan skips `scripts/` directories, so the two anchors of this tool are documented here (a README is read as a contract, which does not need the anchor to be visible in the scan). Tests: `scripts/test_odoo_upgrade.py`.

### The tool `odoo_upgrade.py` [@ANCHOR: distributed_redis_cache:COMM_odoo_upgrade_tool]

**Purpose.** Runs `odoo -u <modules>` on a server with exactly the environment the `odoo` systemd service gets. A bare `sudo -u odoo odoo -u ...` has none of the service's `EnvironmentFile=` settings (`REDIS_USERNAME` and `REDIS_PASSWORD` from the Redis env file, the broker and database credentials), production Redis refuses unauthenticated clients, and every cache invalidation of the upgrade then failed with a warning while the Redis cache kept its pre-upgrade entries. Called by an operator (root) as part of a deploy, after `pagerduty-maintenance start` and `systemctl stop odoo`.

**Interface.** `sudo python3 odoo_upgrade.py --db NAME --modules a,b [--install c,d] [--log FILE] [--dry-run] [--no-css-check]`, or `--db NAME --shell < script.py` for a one-off `odoo shell` in the same environment.

| Option | Meaning |
|---|---|
| `--db` | Database name (required); an invalid name is refused with `UpgradeError`. |
| `--modules` | Comma-separated module names (`[a-z0-9_]+` each) or `all`; required unless `--shell`. |
| `--install` | Comma-separated modules that are new on the database; adds `-i` to the same run (`-u all` does not install a new `auto_install` module). |
| `--log` | Write odoo's output to this file instead of standard output. |
| `--dry-run` | Prints the `systemd-run` command and runs nothing. |
| `--no-css-check` | Skips the stylesheet check (only to recover from a broken checker). |
| `--shell` | Runs `odoo shell` with the script on standard input in the unit `odoo-shell-run`; takes neither `--modules` nor `--install`. |

**Behaviour.** Reads the service's `EnvironmentFiles=`, `Environment=`, `User=` and `Group=` from `systemctl show odoo` and hands them to a transient `systemd-run` unit named `odoo-upgrade` (so systemd, as root, reads the files; their contents never enter this process's output, arguments or log). Refuses to run while the `odoo` service is active, and when no EnvironmentFile defines `REDIS_PASSWORD`. A second upgrade cannot start while one runs (a fixed unit name). After a successful upgrade, before odoo is started again, it compiles every stylesheet bundle in the unit `odoo-upgrade-css-check` (`check_asset_bundles_shell.py` on standard input, nothing written) because Odoo otherwise answers HTTP 200 with a "css error" stub stylesheet and the site is unstyled.

**Exit status.** `0` success (or a dry run); `2` a precondition failed (`UpgradeError`, printed as `refusing: ...`); `3`, or the upgrade's own non-zero status, when the output shows Redis authentication failures; `4` a stylesheet bundle does not compile (the rollback steps are printed; do not start odoo); otherwise the status of `odoo -u`.

**Special concerns.** Needs root; blocking for the length of the upgrade; holds odoo's HTTP port while it runs, so do not `systemctl restart odoo` until `systemctl is-active odoo-upgrade` reports inactive; does not stop or start odoo and does not set the pagerduty maintenance flag. The HTTPS check of the live site (`production_readiness_check.py --site-css-only`) is still the mandatory last step.

### `UpgradeError` [@ANCHOR: distributed_redis_cache:distributed_redis_cache_scripts_odoo_upgrade_upgrade_error]

**Purpose and location.** The exception class `UpgradeError(Exception)` in `scripts/odoo_upgrade.py`, raised when a precondition of the upgrade fails: an invalid `--db`, `--modules` or `--install`, `--shell` combined with `--modules` or `--install`, the `odoo` service still running, a service without a readable EnvironmentFile that defines `REDIS_PASSWORD`, or an undeterminable `User=`.

| Field | Type | Meaning |
|---|---|---|
| message (the one constructor argument) | str | What failed. Safe to print: it never contains a value read from an environment file (only key names, paths and the service name). |

**Invariants and lifetime.** Nothing on the system is changed before it is raised. `main()` catches it, prints `refusing: <message>` to standard error and returns status 2; any other exception is a bug and propagates. It is created, raised and caught inside one single-threaded process, never stored or sent. Raised by `require_redis_credentials`, `build_command` and the argument checks in `run`; reported by `main`.
