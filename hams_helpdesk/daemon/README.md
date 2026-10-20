# Daemon failure reporting (`hams_helpdesk/daemon/`)

Failures inside external daemons (RabbitMQ consumers such as the SES ingest, the ADIF processor,
`dx.firehose`, the club crawlers, the sync daemons, the Rust daemons) happen outside Odoo, so no Odoo
hook sees them. They reach the same server-exception tickets (Helpdesk > Server Exceptions, source
"External daemon") by one authenticated Odoo call. Nothing here deploys itself; an operator installs the
files below.

## How a failure reaches a ticket

1. **systemd `OnFailure=`** (`hams-exception-report@.service`, `hams-exception-report.conf`). When a
   unit that has the drop-in enters the failed state, systemd starts the template unit with the failed
   unit's name as the instance. It runs `exception_reporter.py report-unit <unit>`, which sends the unit
   name, systemd's `Result` word and the last 20 journal lines. A unit with `Restart=always` only enters
   the failed state when its start limit is hit on older systemd, so `OnFailure=` alone reports a daemon that
   has given up (measured on hams1, systemd 257, 2026-10-12: `OnFailure=` already fires on every failed exit there). A self-restarting unit also gets `hams-exception-report-crash.conf`, an `ExecStopPost=-+` line that
   starts the same template when systemd stops the service with a `$SERVICE_RESULT` other than
   `success` (a crash, a signal, a watchdog or timeout; a deploy restart is `success` and stays
   quiet). It runs as root (`+`) so a `hamsd_*` account can start the unit, and `-` keeps a failed
   report from failing the daemon. This is how a `hamsd_*` daemon, which cannot read the reporter key,
   gets a ticket for each crash.
   The report waits out an Odoo that is down or restarting (retries with backoff for up to ten minutes; the template's
   `TimeoutStartSec=900` outlasts that), because a failure during a deploy is exactly when Odoo is unreachable, and an
   undelivered report leaves a failed `hams-exception-report@<unit>.service` that the systemd failed-services pager check reports.
2. **`reporting_failures`** (`exception_reporter.py`, Python, standard library only). A daemon wraps its
   consumer loop: an uncaught `Exception` is reported (the class and the frames, never the message), at
   most once a minute per name in that process, and then raised again, so the daemon still stops and
   systemd still restarts it. Use it on the loop that must not die silently:

   ```python
   from exception_reporter import reporting_failures
   with reporting_failures("ses-inbound-mail-ingest.service"):
       consume_forever()
   ```

   The module lives in this addon; a daemon that runs from another checkout puts this directory on its
   `PYTHONPATH` (the same way `daemons/run_daemon.sh` already adds `daemons/`) or copies the one file.
3. **Rust daemons** do not need a library: the OnFailure unit covers a daemon that has stopped. To
   report a panic in a long-running loop, send the same request the helper sends (below) from a panic
   hook or the supervising task, with `kind: "consumer_exception"`, the frames or error type as
   `journal_tail` and never an error message that can hold data.

## The Odoo side

`hams_helpdesk.exception.fingerprint.report_daemon_failure(unit, kind, result, journal_tail)`, called as
`POST /json/2/hams_helpdesk.exception.fingerprint/report_daemon_failure` with
`Authorization: Bearer <key>`:

* **Authentication.** The key belongs to the `exception_reporter_service_internal` service account,
  issued and rotated by the Daemon Key Manager into `/opt/hams/etc/keys/exception_reporter.env`
  (`ODOO_RPC_LOGIN`, `ODOO_RPC_KEY`, mode 0600, owned by the Odoo user). The method refuses every other
  account. That account can touch only `server_exception` tickets and the fingerprint rows. No secret is
  in code, a unit file or an argument list. The key is registered by `post_init_hook` on a fresh
  install and by `migrations/1.1` on an installed database.
* **Input.** `unit` must be a plain systemd unit name (the instance part of `name@instance.service` is
  dropped); `kind` is `unit_failed` or `consumer_exception`; `result` is one of systemd's Result words,
  else "unknown".
* **Scrubbing.** The journal tail is free text from outside, so every line goes through the strict
  scrubber (emails, URLs, IP addresses, `password=` / `token=` pairs, long tokens, callsigns, long digit
  runs), an exception line keeps only its class, and the tail is cut to 20 lines of 160 characters.
* **Rate limits.** The shared ones: a fingerprint (unit, kind, result) is written at most once a minute
  per process, the whole process 30 times a minute, 50 new tickets an hour; repeats add to the count. The
  template unit also has a start limit (5 in 300 s, so a crash loop is bounded), and the helper sends at most one report a minute per name.

## Installing (operator)

On hams1 nothing is installed by hand: `hams_shared/tools/infrastructure.py` (MANIFEST) writes
`/opt/hams/systemd/hams-exception-report@.service` (linked into `/etc/systemd/system/`) and one
`/etc/systemd/system/<unit>.service.d/hams-exception-report.conf` drop-in for every production service
except the audited exclusions in `EXCEPTION_REPORT_EXCLUDED_UNITS`. The unit runs the deployed file
`/opt/hams/src/hams_open/hams_helpdesk/daemon/exception_reporter.py` (kept current by
`deploy_to_production.py`), and takes `ODOO_URL` and `DB_NAME` from `common.env`. To install a single
entry by hand, render it from the MANIFEST (`hams_shared/tools/render_static_file.py <path>`), then
`systemctl daemon-reload`.

Elsewhere: copy `hams-exception-report@.service` to `/etc/systemd/system/` (and fix its `ExecStart` path),
`hams-exception-report.conf` to `/etc/systemd/system/<daemon>.service.d/`, then `systemctl daemon-reload`.
Test with `systemctl start hams-exception-report@<some-unit>.service`.

## In-process reporting needs the reporter key

`reporting_failures` reads the same key file, `/opt/hams/etc/keys/exception_reporter.env` (override with
`EXCEPTION_REPORTER_KEY_FILE`). It is readable by the `odoo` account only, so only a daemon that runs as
`odoo` (adif.ingress, adif.processor, backup.worker, cache-manager) can deliver an in-process report. A
daemon in its own `hamsd_*` account cannot read it and is covered by the OnFailure unit alone. The helper
never uses `ODOO_KEY_FILE`: a daemon's unit sets that to the daemon's own key, which the endpoint refuses.
A daemon imports the helper from `/opt/hams/daemons/exception_reporter.py`
(`deploy_to_production.py --sync-daemons` copies it there; see `DAEMON_SHARED_FILES`).
