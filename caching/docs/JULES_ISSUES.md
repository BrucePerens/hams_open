# Jules Issues - Caching Module

## Environment & Testing
- Encountered `psycopg2.OperationalError: connection to server on socket "/opt/hams/pgsock/.s.PGSQL.5432" failed: Permission denied` when running tests. Resolved by `sudo chmod 777 /opt/hams/pgsock`.
- Service Worker registration tour passed after ensuring the environment was correctly initialized.

## Future Feature Ideas
- **Offline Mode Support**: Currently, the SW only caches static assets. We could implement a basic "offline" page or cache some critical JSON-RPC responses for read-only offline access.
- **Cache Statistics Dashboard**: Add a view to show total cache size and number of cached assets across all users (anonymized/aggregated).
- **Manual File Blacklist**: Allow administrators to explicitly exclude certain files or patterns from the cache via the UI.
