# Jules Audit Issues for `caching` module

## Resolved Issues

- **Over-privileged Service Account:** The `caching.user_caching_service` had `perm_write` and `perm_create` on `ir.config_parameter`. This was downgraded to read-only as the service account only needs to read `caching.safe_quota_mb` and `caching.invalidation_version`.
- **Hidden File Inclusion:** The filesystem scanner now explicitly ignores hidden files (starting with `.`) in `static/` directories.
- **SW Regex Precision:** The `CACHE_URL_REGEX` in `sw.js` was un-anchored, potentially matching unwanted substrings in URLs. It is now anchored to the start of the path (`^`).

## Observations

- **Tour Robustness:** The tour `caching_service_worker_check` correctly uses a body class `sw-registered` to synchronize with the asynchronous Service Worker registration.
- **Zero-Sudo Compliance:** The module correctly uses `zero_sudo.security.utils` for parameter access and service account escalation.
