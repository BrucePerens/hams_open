# Story: Searching Logs, Opening the Board and the Accounts Behind Them

## Persona
**Dave**, a Pager Duty admin. He searches logs from Odoo, opens the NOC board from an old bookmark, and wants to know that
the accounts that create incidents on the system's behalf can do nothing else.

## Log search

1.  **What can be searched.** The JSON-RPC route `/api/v1/pager/logs/files` (`get_log_files()`) returns the paths of the active
    `pager.log.file` records the caller may see, at most 1000. It runs in the caller's own environment, not as the service
    account, so the website and company rule applies and one tenant cannot learn another tenant's registered paths. A user
    who is not a Pager Duty admin is refused (entry `get_log_files()` below).
2.  **Starting a search.** `/api/v1/pager/logs/search` (`search_logs()`) accepts only a path that is under `/var/log`, has no
    traversal, and is registered in the caller's own scope; it creates a pending `pager.log.search.job`, publishes the request
    to the log daemon over Redis and returns the job id at once instead of blocking. The caller then polls
    `search_logs_poll` (entry `search_logs()` below).

## The board

1.  **Old links keep working.** `/pager/board` is the direct link from before the board became an Odoo backend client action.
    It requires a signed-in user and redirects to `/odoo?action=<id>` of the board action, or to `/odoo` when the action is
    missing (entry `pager_board()` below).
2.  **Fast board data.** The board's numbers come from one stored procedure, `pager_get_board_data(website, company)`, which
    returns the on-duty user, the active incidents (capped at 50), the recently resolved ones and the statistics as JSON.
    `_install_postgres_procedures()` installs it with `CREATE OR REPLACE` from the post-init hook, inside a savepoint, and logs
    and survives a failure so the module still installs (entry `_install_postgres_procedures()` below).

## Inbound mail and the creator account

1.  **Mail becomes incidents.** Mail to the `info@` and `postmaster@` aliases creates incidents whose source is
    `info-email:<sender>` or `postmaster-email:<sender>`, not `email:<sender>`. `_is_inbound_email_source()` recognises
    those sources (the part before the first colon is `email` or ends in `-email`), so the helpdesk adapter's filter for mail
    that is not a monitoring failure actually applies; matching only `email:` left it inert in production and spam stayed in
    the `new` stage. [@ANCHOR: pager_duty:is_inbound_email_source]
2.  **The creator account is narrow.** Incidents are created as `user_pager_incident_creator`, an identity reachable from any
    daemon's report, any inbound email and the relay bridge's theft-case intake. It holds only
    `group_pager_incident_creator`, whose whole grant is read, write and create on `pager.incident`: it cannot unlink an
    incident, cannot read or write checks (heartbeat monitors, certificate domain lists) and cannot see the log files,
    patterns or search jobs. Tests pin the group membership, the exact access rows and rules, and that a direct attempt to
    reach `pager.check` is refused. [@ANCHOR: pager_duty:incident_creator_group]

## Synthetic checks fetching URLs

A synthetic check may download files (`sandbox_downloads`) before running a script. Both the first URL and every redirect
target are checked against the shared SSRF guard before any connection, so a check, however it is configured, cannot be
pointed at the host's own network.

1.  `_is_ssrf_safe_public_ip()` says whether an address is one a server-initiated download may reach: global addresses only,
    refusing loopback, link-local (which covers the cloud metadata address), private ranges, multicast, reserved and the
    unspecified address. It delegates to `zero_sudo`'s shared implementation so the monitor and the binary downloader agree (entry below).
2.  `_assert_host_is_ssrf_safe()` resolves the hostname once and raises a `Security Alert` `ValueError` unless every resolved
    address passes that test (entry below).

## Function reference

### `get_log_files(self)` [@ANCHOR: pager_duty:get_log_files]

The JSON-RPC route `POST /api/v1/pager/logs/files` (`controllers/log_api.py`, `type="jsonrpc"`, `auth="user"`). No arguments besides the standard JSON-RPC envelope (`{"params": {}}`). The caller must be a member of `pager_duty.group_pager_admin`, else `AccessError("Access Denied.")`. It searches the active `pager.log.file` records in the caller's own environment, not as the service account, whose record rule is deliberately unrestricted; so the website and company rule `pager_log_file_website_company_rule` limits the answer to the files the caller's scope registered, at most 1000. Returns `{"files": [path, ...]}`. Read only and unauthenticated callers never reach it. The log viewer page calls it on mount to fill its file list. Test: `test_02b2_log_files_lists_only_the_registered_active_files_and_only_to_an_admin` in `tests/test_controllers.py`.

### `search_logs(self, file_path, regex_query)` [@ANCHOR: pager_duty:search_logs]

The JSON-RPC route `POST /api/v1/pager/logs/search` (`auth="user"`) that starts a search of a registered log file. Arguments: `file_path` (`str`, absolute path) and `regex_query` (`str`, the pattern, run later by the log daemon). Checks, in order: caller is a `group_pager_admin` member (else `AccessError`); no `..` component and the real path (symlinks resolved) is `/var/log` or below; the real path equals one of the active `pager.log.file` paths visible to the caller's own environment, so a path registered for another website is refused. Then it creates a `pager.log.search.job` (`uuid`, state `pending`) and publishes `{"uuid", "file", "regex"}` as JSON on the Redis channel `log_search_req`. Returns `{"job_id": uuid}` at once, or `{"error": ...}` when Redis is unavailable or publishing fails. It does not wait for the result; the caller polls `search_logs_poll`. The daemon (`pager_log_analyzer.py`), chrooted to `/var/log` and unprivileged, does the reading; the pattern is never run inside Odoo. Tests: `test_03_async_bastion_pattern` and `test_05_path_registered_for_another_website_is_rejected` in `tests/test_log_analyzer.py`.

### `pager_board(self, **kw)` [@ANCHOR: pager_duty:pager_board_route]

The legacy page route `GET /pager/board` (`controllers/board.py`, `type="http"`, `auth="user"`, `website=True`), kept so bookmarks from before the board became a backend client action still work. Arguments: any query parameters, ignored. It looks up the action `pager_duty.action_pager_board_client` and redirects (HTTP 302) to `/odoo?action=<id>`; if the action does not exist it redirects to `/odoo`. An anonymous visitor is sent to the login page by Odoo before the method runs. No data is read or written. The website filter is chosen later by the board itself (see `js_board.md`). Tests: `test_02_board_security_and_render` and `test_02a_legacy_board_link_redirects_to_the_backend_client_action` in `tests/test_controllers.py`.

### `_install_postgres_procedures(env)` [@ANCHOR: pager_duty:install_postgres_procedures]

`hooks.py`. Installs the PostgreSQL function `pager_get_board_data(p_website_id, p_company_id)` that the board calls through `get_board_data`. Argument: `env` (an Odoo `Environment`, in the post-init hook). It flushes pending ORM writes (`env.flush_all()`), then, inside a savepoint, runs the `CREATE OR REPLACE FUNCTION` statement held in `_PAGER_BOARD_SQL`. Because it replaces, running it again (module update) installs the current definition. Any exception is logged as an error ("Failed to install pager_get_board_data") and swallowed, so the module install does not fail; the board would then fail until the function exists. Returns nothing. The SQL contains no user input. Test: `test_07b_installing_the_board_procedure_again_replaces_it` in `tests/test_incident.py`.

### `_is_ssrf_safe_public_ip(ip_obj)` [@ANCHOR: pager_duty:synthetic_spooler_is_ssrf_safe_public_ip]

In `daemon/pager_synthetic_spooler.py`: a thin wrapper kept as a real function around `zero_sudo/daemon/ssrf_safe_fetch.is_ssrf_safe_public_ip`. Argument: `ip_obj` (an `ipaddress.IPv4Address` or `IPv6Address`). Returns `True` only when the address is global and is not private, loopback, link-local (this covers the cloud metadata address 169.254.169.254), multicast, reserved or unspecified. Pure function, no I/O, never raises for a valid address object. It is shared so the monitor and the binary downloader apply one rule. Test: `test_is_ssrf_safe_public_ip_classifies_real_addresses_correctly` in `tests/test_synthetic_spooler.py`.

### `_assert_host_is_ssrf_safe(hostname, context)` [@ANCHOR: pager_duty:synthetic_spooler_ssrf_guard]

In `daemon/pager_synthetic_spooler.py`. Arguments: `hostname` (`str`, from the URL) and `context` (`str`, the check name, used in messages). It calls the shared `resolve_ssrf_safe_addresses`, which does one `getaddrinfo` and rejects the hostname if it is empty, cannot be resolved or if any resolved address is not public. A rejection is turned into `ValueError("Security Alert: ...")`. Returns nothing on success. It runs before the first connection and again on the final URL after redirects; the real connection is additionally pinned to the validated addresses by `urlopen_ssrf_safe`, which closes the DNS-rebinding gap that a check alone leaves. The `ValueError` is caught per check by `execute_check` and becomes that check's error, so one bad URL does not stop the spooler. Test: `test_02b_sandbox_downloads_rejects_a_loopback_target`.
