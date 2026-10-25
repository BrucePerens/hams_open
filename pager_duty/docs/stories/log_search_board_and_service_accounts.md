# Story: Searching Logs, Opening the Board and the Accounts Behind Them

## Persona
**Dave**, a Pager Duty admin. He searches logs from Odoo, opens the NOC board from an old bookmark, and wants to know that
the accounts that create incidents on the system's behalf can do nothing else.

## Log search

1.  **What can be searched.** The JSON-RPC route `/api/v1/pager/logs/files` (`get_log_files()`) returns the paths of the active
    `pager.log.file` records the caller may see, at most 1000. It runs in the caller's own environment, not as the service
    account, so the website and company rule applies and one tenant cannot learn another tenant's registered paths. A user
    who is not a Pager Duty admin is refused. [@ANCHOR: pager_duty:get_log_files]
2.  **Starting a search.** `/api/v1/pager/logs/search` (`search_logs()`) accepts only a path that is under `/var/log`, has no
    traversal, and is registered in the caller's own scope; it creates a pending `pager.log.search.job`, publishes the request
    to the log daemon over Redis and returns the job id at once instead of blocking. The caller then polls
    `search_logs_poll`. [@ANCHOR: pager_duty:search_logs]

## The board

1.  **Old links keep working.** `/pager/board` is the direct link from before the board became an Odoo backend client action.
    It requires a signed-in user and redirects to `/odoo?action=<id>` of the board action, or to `/odoo` when the action is
    missing. [@ANCHOR: pager_duty:pager_board_route]
2.  **Fast board data.** The board's numbers come from one stored procedure, `pager_get_board_data(website, company)`, which
    returns the on-duty user, the active incidents (capped at 50), the recently resolved ones and the statistics as JSON.
    `_install_postgres_procedures()` installs it with `CREATE OR REPLACE` from the post-init hook, inside a savepoint, and logs
    and survives a failure so the module still installs. [@ANCHOR: pager_duty:install_postgres_procedures]

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
    unspecified address. It delegates to `zero_sudo`'s shared implementation so the monitor and the binary downloader agree.
    [@ANCHOR: pager_duty:synthetic_spooler_is_ssrf_safe_public_ip]
2.  `_assert_host_is_ssrf_safe()` resolves the hostname once and raises a `Security Alert` `ValueError` unless every resolved
    address passes that test. [@ANCHOR: pager_duty:synthetic_spooler_ssrf_guard]
