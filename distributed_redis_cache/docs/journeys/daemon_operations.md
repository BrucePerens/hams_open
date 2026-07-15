<!-- Copyright © Bruce Perens K6BP. AGPL-3.0. -->

# Journey: Daemon Operations

This journey describes the lifecycle and configuration of the `cache_manager.py` daemon.

1. **Environment Setup**: The daemon reads its configuration ([@ANCHOR: COMM_cache_manager_config]) from environment variables (e.g., `REDIS_HOST`, `DB_NAME`).
2. **Initialization**: The daemon starts up and establishes an asynchronous connection to both Redis and PostgreSQL.
3. **Health Check**: It pings Redis to ensure connectivity before proceeding ([@ANCHOR: COMM_check_redis_status_logic]).
4. **Main Loop**:
   - The daemon enters a persistent loop, using `asyncpg` to maintain a non-blocking `LISTEN` state on the database.
   - It implements a reconnection strategy, waiting 5 seconds before retrying if the PostgreSQL connection drops.
5. **Shutdown**: Upon receiving a termination signal (SIGINT/SIGTERM), the daemon closes its connections cleanly to prevent resource leaks.
