# Story: Hearing About a Certificate or a Disk Before It Fails

## Persona
**Bob**, who runs the hams.com host and wants the monitor to warn him about two slow failures long before they stop anything:
the auth.hams.com certificate that the certificate-login gateway serves, and the disk that the QSO recordings fill.

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
    shared ladder used for tokens would only reach `medium` at 14 days, which does not page, so this check has its own.
    [@ANCHOR: pager_duty:auth_cert_severity]
2.  **Reading the expiry.** `parse_enddate()` turns the `notAfter=Dec 20 03:04:05 2026 GMT` line of `openssl x509 -enddate`
    into a UTC time, including openssl's space-padded single-digit days, and refuses output that does not start with
    `notAfter=`. [@ANCHOR: pager_duty:auth_cert_parse_enddate]
3.  **File first, server second.** `read_expiry()` returns the expiry and where it came from: the file when it can be opened,
    otherwise the certificate the server presents on `auth.hams.com:443` (read without verification, since only its dates are
    wanted). [@ANCHOR: pager_duty:auth_cert_read_expiry]
4.  **The check itself.** `main()` reads the path, host, port and thresholds from the `HAMS_AUTH_CERT_*` environment
    variables, and reports `SEVERITY:high` when the certificate cannot be read at all, since an unreadable certificate is
    more urgent than a near one. Otherwise it prints the days left and exits 1 with the severity from the ladder, or exits 0
    with a healthy message. [@ANCHOR: pager_duty:auth_cert_main]

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
