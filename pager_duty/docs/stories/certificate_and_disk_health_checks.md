# Story: Hearing About a Certificate or a Disk Before It Fails

## Persona
**Bob**, who runs the hams.com host and wants the monitor to warn him about two slow failures long before they stop anything:
the auth.hams.com certificate that the certificate-login gateway serves, and the disk that the QSO (radio contact) recordings fill.

## Context
Both checks are standalone scripts that a `pager.check` of type "synthetic" runs. Each exits 0 when healthy and 1 when failing,
and when failing prints a bare `SEVERITY:<level>` line on stderr, which `generalized_monitor.py` reads to choose the
incident's severity. Low and medium incidents are tracked as trends and do not page on-call; high and critical page.

## The auth.hams.com certificate

`check_auth_cert_expiry.py` looks at the certificate file a renewal job installs (`/etc/hams/auth/auth.crt`), because the
file is the thing that stops being renewed when the job breaks. When the account running the check cannot read the file
(the directory is `0750 root:hams-auth`), it asks the live server for its certificate instead, which is what a visitor would
get. No private key is ever read.

1.  **Severity ladder.** `severity_for_auth_cert()` turns the days left into a level: nothing at 30 days or more, `low` under
    30 days (recorded, not paged), `high` under 14 days (pages on-call) and `critical` under 2 days or already expired. The
    shared ladder used for tokens would only reach `medium` at 14 days, which does not page, so this check has its own
    (entry `severity_for_auth_cert` below).
2.  **Reading the expiry.** `parse_enddate()` turns the `notAfter=Dec 20 03:04:05 2026 GMT` line of `openssl x509 -enddate`
    into a UTC time, including openssl's space-padded single-digit days, and refuses output that does not start with
    `notAfter=` (entry `parse_enddate` below).
3.  **File first, server second.** `read_expiry()` returns the expiry and where it came from: the file when it can be opened,
    otherwise the certificate the server presents on `auth.hams.com:443` (read without verification, since only its dates are
    wanted) (entry `read_expiry` below).
4.  **The check itself.** `main()` reads the path, host, port and thresholds from the `HAMS_AUTH_CERT_*` environment
    variables, and reports `SEVERITY:high` when the certificate cannot be read at all, since an unreadable certificate is
    more urgent than a near one. Otherwise it prints the days left and exits 1 with the severity from the ladder, or exits 0
    with a healthy message (entry `main` below).

`check_auth_cert_expiry.py --profile turn` (entry `--profile turn` below) does the same for the members-only TURN (Traversal Using Relays around network address translation) server's certificate (`/etc/hams/turn/turn.crt`, served by
coturn as turn.hams.com, renewed by `hams-turn-cert-renew.timer`), with the same warning and paging ladder and `HAMS_TURN_CERT_*` overrides.

## The recordings disk

`check_recordings_disk_usage.py` exists because the built-in disk check only compares with its critical threshold and cannot
warn early, and recordings are never deleted, so hearing about it in time is the only protection against a full disk. It
also estimates how long that time is.

1.  **Severity.** `severity_for_percent()` gives nothing below the warning percent (default 70), `medium` from there up to the
    failure percent (default 85), recorded as a trend, and `high` at or above it, which pages.
    [@ANCHOR: pager_duty:recordings_disk_severity]
2.  **Filesystem use.** `filesystem_usage()` returns the percent used and the used and available bytes of the filesystem that
    holds the directory, the way `df` computes "Use%" (used over used plus what a non-root user can still write). It is the
    filesystem that is judged, not the directory, because the directory shares `/` with everything else. A directory that does
    not exist yet is measured through its nearest existing parent, so the check works before recording starts.
    [@ANCHOR: pager_duty:recordings_disk_usage]
3.  **Volume.** `directory_volume()` counts the regular files directly in the directory and in its `sessions/` folder and
    adds up their sizes, including the bytes modified in the last 7 days. It only lists names and sizes and never opens a
    recording, and it stops after a time budget (and says so) so a huge directory cannot make the monitor's own timeout fail
    the check. [@ANCHOR: pager_duty:recordings_volume]
4.  **Growth estimate.** `estimate()` turns the recent bytes into a rate per day (averaged over the directory's own age, at
    least one hour, when it is younger than the 7-day window, so the first day does not read as no growth) and, with the free
    space, into the days left until the failure percent. It reports 0 days when the line is already crossed and no figure when
    nothing is growing. [@ANCHOR: pager_duty:recordings_estimate]
5.  **The check itself.** `main()` takes the directory, `--warn` and `--fail`. A failing run prints `SEVERITY:<level>` and a
    message whose first 80 characters carry the essentials, because the monitor keeps only the first 100 characters of stderr;
    a healthy run prints the full estimate line. A filesystem that cannot be measured is reported as `high`.
    [@ANCHOR: pager_duty:recordings_disk_main]

## Function reference: the certificate check

All four functions are in `daemon/check_auth_cert_expiry.py`. Tests are in `tests/test_check_auth_cert_expiry.py`.

### `severity_for_auth_cert(days_left, warn_days=30, page_days=14)` [@ANCHOR: pager_duty:auth_cert_severity]

Maps the time left on a certificate to a severity. `days_left` is a number of days, possibly fractional or negative (already expired); `warn_days` and `page_days` are the two thresholds (30 and 14 by default, set from the environment by `main`). It returns `"critical"` when `days_left` is under 2 (`CRITICAL_DAYS`, which includes expired), `"high"` under `page_days`, `"low"` under `warn_days`, else `None` (healthy). `high` and `critical` page on-call; `low` is recorded and tracked as a trend but does not page. This own ladder exists because the shared `severity_for_days_left` would give `medium` at 14 days, which does not page. Pure function, never raises. Tests: `test_healthy_at_and_past_thirty_days`, `test_warning_under_thirty_days_does_not_page`, `test_pages_under_fourteen_days`, `test_critical_under_two_days_and_when_expired`.

### `parse_enddate(output)` [@ANCHOR: pager_duty:auth_cert_parse_enddate]

Reads the output of `openssl x509 -noout -enddate`. `output` is the text, whose first line must be `notAfter=Dec 20 03:04:05 2026 GMT`. It collapses repeated spaces first, because openssl pads a one-digit day with a space (`Jan  5`), and parses with `%b %d %H:%M:%S %Y %Z`. Returns a timezone-aware `datetime` in UTC. Raises `ValueError("unexpected openssl output: ...")` (first 60 characters) when the first line does not start with `notAfter=`, when the output is empty, and `ValueError` from `strptime` for a date it cannot read. Pure function. Test: `test_parse_enddate_handles_openssl_padding`.

### `read_expiry(path, host, port)` [@ANCHOR: pager_duty:auth_cert_read_expiry]

Finds the certificate's expiry from the best source. Arguments: `path` (`str`, the certificate file), `host` (`str`) and `port` (`int`) of the live server. It first tries to open the file (only to check it is readable; the private key is never touched). If that works it asks openssl about the file (`_enddate_of(["-in", path])`) and returns `(expiry, path)`. If opening fails with `OSError` (for example the directory is `0750 root:hams-auth` and the check runs as another account) it fetches the server's certificate with `ssl.get_server_certificate((host, port), timeout=10)`, without verification since only the dates matter, and returns `(expiry, "served by host:port")`. Raises what the helpers raise: `ValueError` for unreadable certificate text, `OSError`, `ssl.SSLError` or `socket.timeout` for the network. Blocks up to 10 s for the network and 15 s for openssl. Tests: `test_missing_file_falls_back_to_the_served_certificate`, `test_file_and_network_both_unavailable_pages`.

### `main(argv=None)` [@ANCHOR: pager_duty:auth_cert_main]

The check entry point. `argv` is the argument list (default `sys.argv[1:]`); the only option is `--profile auth|turn`. Settings come from the environment with the prefix of the profile (`HAMS_AUTH_CERT_` or `HAMS_TURN_CERT_`): `PATH`, `HOST`, `PORT`, `WARN_DAYS`, `PAGE_DAYS`. Returns the process exit status: 0 healthy (prints "<host> certificate healthy, expires in N days (date, source)"); 1 failing (prints `SEVERITY:<level>` and then the message on stderr, which the monitor reads); 2 for an unknown profile. If the certificate cannot be read at all it prints `SEVERITY:high` and "<profile> certificate unreadable: ...", because having no expiry is more urgent than a near one. It reads one local file or makes one TLS (Transport Layer Security) connection, and logs nothing secret. Tests: `test_healthy_certificate_exits_zero`, `test_under_thirty_days_warns_with_low_severity`, `test_under_fourteen_days_pages_with_high_severity`, `test_an_unreadable_certificate_pages`.

### `--profile turn` of `main` [@ANCHOR: pager_duty:auth_cert_main]

The same function, selected with `check_auth_cert_expiry.py --profile turn`, watches the certificate of the members-only TURN server instead: file `/etc/hams/turn/turn.crt`, host `turn.hams.com`, environment prefix `HAMS_TURN_CERT_`. The certificate is served by coturn and renewed by `hams-turn-cert-renew.timer`. The profile table `PROFILES` holds the prefix, default path and host of each profile; the thresholds, the severity ladder and the file-then-server fallback are identical. An unknown profile name exits 2 with a message listing the valid ones. The message names the profile's host, so the incident says `turn.hams.com certificate ...`. Tests: `test_the_turn_profile_reads_its_own_certificate_and_names_it`, `test_the_turn_profile_pages_on_its_own_ladder`, `test_an_unknown_profile_is_refused`.
