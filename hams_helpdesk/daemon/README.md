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
   the failed state when its start limit is hit, so this reports a daemon that has given up, not every
   crash.
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
  template unit also has a start limit, and the helper sends at most one report a minute per name.

## Installing (operator)

```
install -m 0644 hams-exception-report@.service /etc/systemd/system/
install -d /etc/systemd/system/<daemon>.service.d
install -m 0644 hams-exception-report.conf /etc/systemd/system/<daemon>.service.d/
systemctl daemon-reload
```

Set `ODOO_URL` (and `ODOO_DB` for a multi-database server) in a drop-in on the template unit if Odoo is
not at `http://odoo:8069`. Test with `systemctl start hams-exception-report@<some-unit>.service`.
