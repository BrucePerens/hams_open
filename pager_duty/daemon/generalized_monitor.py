# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
import binascii
import concurrent.futures
import datetime
import json
import logging
import os
import psutil
import re
import secrets
import shutil
import smtplib
import socket
import ssl
import subprocess
import sys
import time
import urllib.request
import urllib.error
import ftplib
import imaplib
import poplib
import ldap3
import ntplib
import shlex
import xmlrpc.client
from email.message import EmailMessage

import psycopg2
import pymysql
import redis as redis_lib

# The pagerduty maintenance flag (/etc/pagerduty/maintenance). The sibling file is imported by plain name from this
# directory, which works both when the daemon runs as a script from here and when Odoo's tests import this module.
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import pagerduty_maintenance  # noqa: E402

# Real fix, found by an adversarial security review: this module's own
# internal Redis connections (log_anomaly_proxy/log_search_proxy below)
# used to construct bare redis.Redis(host=..., port=..., db=0) with no
# password at all, regardless of whether one is configured -- unlike
# distributed_redis_cache/daemons/cache_manager.py's own already-correct
# REDIS_PASS handling, which this mirrors. The admin-only
# /api/v1/pager/logs/search HTTP endpoint (log_api.py) is gated behind
# pager_duty.group_pager_admin, but that check has no effect at all if
# anything able to reach this Redis instance can just publish to the
# same channels directly -- passing a real password here (when one is
# configured) closes that gap rather than leaving the entire access
# boundary for an admin-gated feature resting on Redis network
# reachability alone.
REDIS_PASS = os.getenv("REDIS_PASSWORD") or os.getenv("redis_password")
# The production ACL user (redis.env); unset on a test box, where Redis needs no AUTH.
REDIS_USERNAME = os.getenv("REDIS_USERNAME") or None


class OdooClient:
    # [@ANCHOR: pager_duty:odoo_client_init]
    def __init__(self, url, db, user, password):
        self.url = url.rstrip("/")
        self.db = db
        self.headers = {
            "Authorization": f"bearer {password}",
            "X-Odoo-Database": db,
            "Content-Type": "application/json",
            "User-Agent": "Pager-Daemon/1.0",
        }

    # [@ANCHOR: pager_duty:odoo_client_execute]
    def execute(self, model, method, **kwargs):
        req = urllib.request.Request(
            f"{self.url}/json/2/{model}/{method}",
            data=json.dumps(kwargs).encode("utf-8"),
            headers=self.headers,
            method="POST",
        )
        try:
            with urllib.request.urlopen(req, timeout=15) as response:
                return json.loads(response.read().decode("utf-8"))
        except urllib.error.HTTPError as e:
            err_body = e.read().decode("utf-8")
            raise Exception(f"JSON-2 API Error {e.code}: {err_body}")


# [@ANCHOR: pager_duty:get_odoo_client]
def get_odoo_client(logger, config):
    url = (os.environ.get("ODOO_URL") or "http://odoo:8069").rstrip("/")
    db = config.get("odoo_database") or os.environ.get("ODOO_DB")
    if not db:
        try:
            req = urllib.request.Request(
                f"{url}/web/database/list",
                data=json.dumps({}).encode("utf-8"),
                headers={"Content-Type": "application/json-rpc"},
                method="POST",
            )
            with urllib.request.urlopen(req, timeout=5) as res:
                data = json.loads(res.read().decode("utf-8"))
                dbs = data.get("result", [])
                if dbs:
                    db = dbs[0]
        except (urllib.error.URLError, json.JSONDecodeError, socket.timeout) as e:
            logger.warning("Failed to query Odoo databases (Network/JSON): %s", e)
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.error("Unexpected error querying Odoo databases: %s", e)
    if not db:
        db = "odoo"

    user = os.environ.get("ODOO_USER") or "pager_service_internal"
    # Real bug, found by bug-hunt review (tier 1): this used to silently
    # fall back to an empty-string password when ODOO_PASSWORD wasn't set
    # (e.g. a typo'd var name or a missing line in /etc/pager_daemons.env),
    # instead of failing loudly -- the exact anti-pattern
    # pager_mcp_server.py's own _get_client() explicitly calls out and
    # avoids for this same credential category ("a missing key must fail
    # loudly, not silently degrade to an unauthenticated ... call"). This
    # is the main paging daemon's own Odoo client: a bad/empty credential
    # here means every check thread's status write and every incident
    # report fails, but each of those call sites' own broad
    # "audit-ignore-catch-all" except just logs a warning and carries on --
    # so a misconfigured password would have silently disabled all paging
    # instead of halting the daemon at boot the way a missing dependency or
    # a missing config file already does.
    password = os.environ.get("ODOO_PASSWORD")
    if not password:
        logger.critical(
            "ODOO_PASSWORD is not set -- refusing to start with an "
            "unauthenticated Odoo client (would silently disable all "
            "incident reporting)."
        )
        return None
    try:
        return OdooClient(url, db, user, password)
    except (ConnectionError, socket.timeout, Exception) as e:  # audit-ignore-catch-all
        logger.error(f"Failed to connect to Odoo: {e}")
        return None


logging.basicConfig(
    level=logging.INFO, format="%(asctime)s - [%(levelname)s] - %(message)s"
)
logger = logging.getLogger("generalized_monitor")


# [@ANCHOR: pager_duty:parse_env]
def parse_env(val):
    if isinstance(val, str) and val.startswith("ENV:"):
        return os.environ.get(val[4:]) or ""
    return val


# [@ANCHOR: pager_duty:ensure_executable]
def ensure_executable(cmd_name):
    path = shutil.which(cmd_name)
    if path:
        return path, ""
    return None, f"Missing dependency: '{cmd_name}'. Startup verification failed."


def verify_and_install_dependencies(client, checks):
    # [@ANCHOR: daemon_verify_dependencies]
    type_to_cmd = {
        "dns": "dig",
        "snmp": "snmpget",
        "pg_dump": "pg_dump",
        "nginx": "nginx",
        "certbot": "certbot",
        "logrotate": "logrotate",
        "http3": "curl",
        "icmp": "ping",
        "docker": "docker",
        "systemd": "systemctl",
        "cloudflared": "cloudflared",
    }

    required_cmds = set()
    for check in checks:
        ctype = check.get("type")
        if ctype in type_to_cmd:
            required_cmds.add(type_to_cmd[ctype])

    for cmd in required_cmds:
        if not shutil.which(cmd):
            logger.info(f"Dependency '{cmd}' missing. Polling Odoo...")
            success = False
            for attempt in range(12):
                try:
                    res = client.execute(
                        "pager.check", "rpc_ensure_executable", cmd_name=cmd
                    )
                    if res and res.get("status") == "ok":
                        bin_path = res.get("path")
                        logger.info(f"Provisioned {cmd} at {bin_path}")
                        bin_dir = os.path.dirname(bin_path)
                        if bin_dir not in os.environ["PATH"]:
                            os.environ["PATH"] = (
                                bin_dir + os.pathsep + os.environ["PATH"]
                            )
                        success = True
                        break
                    else:
                        err_msg = res.get("message") if res else "Unknown error"
                        logger.warning(f"Provision failed: {err_msg}")
                except (
                    ConnectionError,
                    socket.timeout,
                    Exception,
                ) as e:  # audit-ignore-catch-all
                    logger.warning(f"RPC unavailable, waiting... ({e})")
                time.sleep(10)  # audit-ignore-sleep

            if not success:
                # Real bug, found by bug-hunt review (tier 1): the
                # report-incident-and-exit block below used to be at this
                # same indentation level as this `if not success:` guard
                # (a sibling, not nested inside it) -- so it ran
                # unconditionally, even when the retry loop above *did*
                # eventually provision the dependency (success=True). On
                # the success path `msg` was never assigned, so the
                # report_incident call raised a bare NameError -- silently
                # swallowed by the broad "audit-ignore-catch-all" except
                # right below it and logged as a misleading "Failed to
                # report ... via RPC" warning -- and then `sys.exit(1)`
                # still ran regardless, killing the daemon (systemd would
                # restart it) immediately after it had *successfully*
                # resolved its own missing dependency. Nesting this block
                # inside `if not success:` makes it run only on the actual
                # failure path it was written for.
                msg = f"FATAL: Missing dependency '{cmd}'. Halting."
                logger.critical(msg)
                fallback_notify("Daemon Boot", msg, "critical")
                try:
                    client.execute(
                        "pager.incident",
                        "report_incident",
                        vals={
                            "source": "Daemon Boot",
                            "severity": "critical",
                            "description": msg,
                        },
                    )
                except (
                    ConnectionError,
                    socket.timeout,
                    Exception,
                ) as e:  # audit-ignore-catch-all
                    logger.warning(
                        "Failed to report missing dependency incident via RPC: %s", e
                    )
                sys.exit(1)


# All three are keyed by check_key(check): the check's record id, which is unique across the whole exported
# config. A check name is only unique per website (pager.check UNIQUE(name, website_id)), so two websites can each
# have a check called "Disk"; keyed by name they shared one heartbeat (a hung thread for one site was hidden by the
# healthy thread for the other), one timeout, and one failing flag (one site's failure suppressed, or cleared, the
# other site's children). Config files written before the id was exported carry names only, and fall back to them.
THREAD_HEARTBEATS = {}
THREAD_TIMEOUTS = {}
FAILING_CHECKS = set()
# key -> name, for log lines only.
THREAD_NAMES = {}
# Names of failing checks. Used only to resolve `parent` in an older config file that has no `parent_id`.
FAILING_NAMES = set()


# [@ANCHOR: pager_duty:check_key]
# Verified by [@ANCHOR: test_20b_two_checks_with_the_same_name_on_different_websites_get_distinct_daemon_state]
def check_key(check):
    """The key this check's daemon state lives under: its record id, else its name (an older config file)."""
    return check.get("id") or check.get("name", "Unknown")


# [@ANCHOR: pager_duty:parent_suppresses]
# Verified by [@ANCHOR: test_20c_a_parent_failing_on_one_website_does_not_suppress_or_clear_the_other_websites_state]
# Verified by [@ANCHOR: test_20d_a_config_from_before_parent_id_still_matches_the_parent_by_name]
def parent_suppresses(check):
    """True when this check's parent is currently failing. A `parent_id` (the parent's record id) is matched by id, so
    a same-named check on another website cannot suppress it; an older config with only the parent's name is matched
    by name, as before."""
    parent_id = check.get("parent_id")
    if parent_id:
        return parent_id in FAILING_CHECKS
    parent = check.get("parent")
    return bool(parent) and parent in FAILING_NAMES


# [@ANCHOR: pager_duty:is_in_maintenance]
def is_in_maintenance(check):
    maint_start_str = check.get("maint_start")
    maint_end_str = check.get("maint_end")
    if maint_start_str and maint_end_str:
        try:
            start = datetime.datetime.strptime(maint_start_str, "%Y-%m-%d %H:%M:%S")
            end = datetime.datetime.strptime(maint_end_str, "%Y-%m-%d %H:%M:%S")
            now = datetime.datetime.utcnow()
            if start <= now <= end:
                return True
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("Maintenance time parse error: %s", e)
    return False


# [@ANCHOR: pager_duty:fallback_notify]
def fallback_notify(source, msg, severity):
    # During pagerduty maintenance (a planned restart; `pagerduty-maintenance start`) the Odoo outage is expected, so
    # the e-mail is not sent. The incident goes to the log at warning. A malformed flag is not maintenance.
    flag = pagerduty_maintenance.read_flag()
    if flag["state"] == pagerduty_maintenance.MALFORMED:
        logger.error("pagerduty maintenance flag ignored, so the page is sent: %s", flag["error"])
    elif flag["state"] == pagerduty_maintenance.ACTIVE:
        logger.warning(
            "pagerduty maintenance active (%s): SMTP fallback e-mail NOT sent. %s - %s",
            pagerduty_maintenance.describe(flag), source, msg,
        )
        return
    fallback_email = os.environ.get("PAGER_FALLBACK_EMAIL")
    smtp_host = os.environ.get("SMTP_HOST")
    smtp_port = int(os.environ.get("SMTP_PORT") or 587)
    smtp_user = os.environ.get("SMTP_USER")
    smtp_pass = os.environ.get("SMTP_PASS")
    from_email = (
        os.environ.get("SMTP_FROM") or "pager-daemon@example.com"
    )

    if not fallback_email or not smtp_host:
        logger.critical(
            f"SMTP Fallback not configured! Incident lost: {source} - {msg}"
        )
        return

    try:
        em = EmailMessage()
        em.set_content(
            f"CRITICAL INCIDENT: {source}\nSeverity: {severity}\nDetails: {msg}\n\n(Sent via Daemon SMTP Fallback because Odoo RPC failed.)"
        )
        em["Subject"] = f"[Pager Alert] {source}"
        em["From"] = from_email
        em["To"] = fallback_email

        with smtplib.SMTP(smtp_host, smtp_port, timeout=10) as server:
            if smtp_port in (587, 465):
                server.starttls()
            if smtp_user and smtp_pass:
                server.login(smtp_user, smtp_pass)
            server.send_message(em)
        logger.info("Successfully dispatched SMTP fallback email.")
    except (ConnectionError, socket.timeout, Exception) as e:  # audit-ignore-catch-all
        logger.critical(f"SMTP Fallback completely failed: {e}")


# [@ANCHOR: pager_duty:severity_for_days_left]
# Graduated thresholds, in days, ordered from least to most urgent -- the first one a credential's
# remaining lifetime falls under (days_left <= threshold) wins. Shared by every expiring-credential
# synthetic check in this codebase (check_github_pat_expiry.py, check_cloudflare_token_expiry.py)
# so they escalate identically rather than each hand-rolling its own ladder.
SEVERITY_THRESHOLDS_DAYS = (
    (2, "critical"),
    (7, "high"),
    (14, "medium"),
    (30, "low"),
)


def severity_for_days_left(days_left):
    """Returns the graduated severity name for `days_left` remaining until some credential
    expires, or None if it's healthy (past every threshold)."""
    for threshold, severity in SEVERITY_THRESHOLDS_DAYS:
        if days_left <= threshold:
            return severity
    return None


# [@ANCHOR: pager_duty:extract_severity_prefix]
_SEVERITY_PREFIX_RE = re.compile(r"^\[SEVERITY:(low|medium|high|critical)\] (.*)$", re.DOTALL)


def extract_severity_prefix(msg, default="high"):
    """The other half of the "synthetic" check type's graduated-severity extension (see
    execute_check()'s own comment on the ctype == "synthetic" branch): pulls a
    "[SEVERITY:xxx] " prefix a synthetic script chose to attach back off of `msg`, returning
    `(severity, cleaned_msg)`. A message with no such prefix (every check type except a
    graduated synthetic one, and every synthetic script that predates this feature) returns
    `default` unchanged and the message untouched -- this is purely additive."""
    match = _SEVERITY_PREFIX_RE.match(msg)
    if not match:
        return default, msg
    return match.group(1), match.group(2)


def report(client, source, msg, severity="high", website_id=False):
    # [@ANCHOR: daemon_report_incident]
    webhook_url = os.environ.get("PAGER_WEBHOOK_URL")
    if webhook_url:
        try:
            payload = {
                "content": f"🚨 **[PAGER ALERT]**\n**Source:** {source}\n**Severity:** {severity}\n**Details:** {msg}"
            }
            req = urllib.request.Request(
                webhook_url,
                data=json.dumps(payload).encode("utf-8"),
                headers={"Content-Type": "application/json"},
                method="POST",
            )
            with urllib.request.urlopen(req, timeout=5):
                pass
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("Webhook dispatch failed: %s", e)

    try:
        payload = {"source": source, "description": msg, "severity": severity}
        if website_id:
            payload["website_id"] = website_id
        client.execute("pager.incident", "report_incident", vals=payload)
        logger.error(f"Incident reported [{source}]: {msg} (Website: {website_id})")
    except (ConnectionError, socket.timeout, Exception) as e:  # audit-ignore-catch-all
        logger.error(
            f"Failed to report incident via RPC: {e}. Triggering SMTP fallback."
        )
        fallback_notify(source, msg, severity)


# [@ANCHOR: pager_duty:auto_resolve]
def auto_resolve(client, source, website_id=False):
    try:
        client.execute(
            "pager.incident",
            "auto_resolve_incidents",
            source=source,
            context={"website_id": website_id} if website_id else {},
        )
        logger.info(
            f"[{source}] System stable. Auto-resolved open incidents. (Website: {website_id})"
        )
    except (ConnectionError, socket.timeout, Exception) as e:  # audit-ignore-catch-all
        logger.error(f"Failed to auto-resolve incidents for {source}: {e}")


def execute_check(check, client=None):
    # [@ANCHOR: daemon_execute_check]
    ctype = check.get("type")
    target = parse_env(check.get("target", ""))

    if ctype == "system":
        if target == "disk":
            part = parse_env(check.get("partition", "/"))
            try:
                pct = psutil.disk_usage(part).percent
                if pct > check.get("critical", 90):
                    return False, f"Disk space at {pct}% on {part}"
            except (
                ConnectionError,
                socket.timeout,
                Exception,
            ) as e:  # audit-ignore-catch-all
                logger.warning("Disk check failed: %s", e)
                return False, f"Disk check failed for {part}: {e}"
        elif target == "memory":
            pct = psutil.virtual_memory().percent
            if pct > check.get("critical", 90):
                return False, f"Memory usage at {pct}%"
        elif target == "cpu":
            pct = psutil.cpu_percent(interval=1)
            if pct > check.get("critical", 90):
                return False, f"CPU usage at {pct}%"
        elif target == "iowait":
            cpu_times = psutil.cpu_times_percent(interval=1)
            pct = getattr(cpu_times, "iowait", 0)
            if pct > check.get("critical", 90):
                return False, f"CPU IO Wait at {pct}%"
        elif target == "steal":
            cpu_times = psutil.cpu_times_percent(interval=1)
            pct = getattr(cpu_times, "steal", 0)
            if pct > check.get("critical", 90):
                return False, f"CPU Steal at {pct}%"
        return True, "OK"

    elif ctype == "load":
        try:
            load1, load5, load15 = os.getloadavg()
            crit = check.get("critical", 0)
            if crit > 0 and load1 > crit:
                return False, f"Load average {load1:.2f} exceeds {crit}"
            return True, f"OK (Load: {load1:.2f})"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("Load check failed: %s", e)
            return False, f"Load check failed: {e}"

    elif ctype == "ftp":
        port = int(parse_env(check.get("port", 21)))
        user = parse_env(check.get("user", ""))
        password = parse_env(check.get("password", ""))
        try:
            with ftplib.FTP() as ftp:
                ftp.connect(target, port, timeout=5)
                if user and password:
                    ftp.login(user, password)
                else:
                    ftp.login()
            return True, "OK"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("FTP check failed: %s", e)
            return False, f"FTP check failed: {e}"

    elif ctype == "imap":
        port = int(parse_env(check.get("port", 143)))
        user = parse_env(check.get("user", ""))
        password = parse_env(check.get("password", ""))
        try:
            if port == 993:
                imap = imaplib.IMAP4_SSL(target, port, timeout=5)
            else:
                imap = imaplib.IMAP4(target, port, timeout=5)
            if user and password:
                imap.login(user, password)
            imap.logout()
            return True, "OK"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("IMAP check failed: %s", e)
            return False, f"IMAP check failed: {e}"

    elif ctype == "pop3":
        port = int(parse_env(check.get("port", 110)))
        user = parse_env(check.get("user", ""))
        password = parse_env(check.get("password", ""))
        try:
            if port == 995:
                pop = poplib.POP3_SSL(target, port, timeout=5)
            else:
                pop = poplib.POP3(target, port, timeout=5)
            if user and password:
                pop.user(user)
                pop.pass_(password)
            pop.quit()
            return True, "OK"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("POP3 check failed: %s", e)
            return False, f"POP3 check failed: {e}"

    elif ctype == "mysql":
        port = int(parse_env(check.get("port", 3306)))
        user = parse_env(check.get("user", ""))
        password = parse_env(check.get("password", ""))
        dbname = parse_env(check.get("dbname", ""))
        try:
            conn = pymysql.connect(
                host=target,
                port=port,
                user=user,
                password=password,
                database=dbname,
                connect_timeout=5,
            )
            try:
                with conn.cursor() as cur:
                    cur.execute("SELECT 1;")
                    cur.fetchone()
            finally:
                conn.close()
            return True, "OK"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("MySQL check failed: %s", e)
            return False, f"MySQL/MariaDB check failed: {e}"

    elif ctype == "ldap":
        port = int(parse_env(check.get("port", 389)))
        try:
            server = ldap3.Server(
                target, port=port, get_info=ldap3.ALL, connect_timeout=5
            )
            conn = ldap3.Connection(server, auto_bind=True, receive_timeout=5)
            conn.unbind()
            return True, "OK"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("LDAP check failed: %s", e)
            return False, f"LDAP check failed: {e}"

    elif ctype == "ntp":
        port = int(parse_env(check.get("port", 123)))
        try:
            client_ntp = ntplib.NTPClient()
            response = client_ntp.request(target, version=3, timeout=5)
            return True, f"OK (Offset: {response.offset:.4f}s)"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("NTP check failed: %s", e)
            return False, f"NTP check failed: {e}"

    elif ctype == "snmp":
        community = parse_env(check.get("snmp_community", "public"))
        oid = parse_env(check.get("snmp_oid", ""))
        exe, err = ensure_executable("snmpget")
        if not exe:
            return False, err
        try:
            res = subprocess.run(
                [exe, "-v2c", "-c", community, target, oid],
                shell=False,
                capture_output=True,
                text=True,
                timeout=5,
            )
            if res.returncode != 0:
                return (
                    False,
                    f"SNMP failed: {res.stderr.strip() or res.stdout.strip()[:100]}",
                )
            expect = parse_env(check.get("expect"))
            if expect and expect not in res.stdout:
                return False, "SNMP payload mismatch"
            return True, "OK"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("SNMP check failed: %s", e)
            return False, f"SNMP check error: {e}"

    elif ctype == "dns":
        domain = target
        exe, _ = ensure_executable("dig")
        try:
            if exe:
                result = subprocess.run(
                    [exe, "+trace", domain], capture_output=True, text=True, timeout=10
                )
                if result.returncode == 0 and domain in result.stdout:
                    return True, "OK"
            socket.gethostbyname(domain)
            return True, "OK"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("DNS check failed: %s", e)
            return False, f"DNS resolution failed: {e}"

    elif ctype == "http":
        try:
            headers = {"User-Agent": "Pager-Daemon/1.0"}
            req = urllib.request.Request(target, headers=headers)
            with urllib.request.urlopen(req, timeout=5) as response:
                if response.status == 200:
                    body = response.read().decode("utf-8")
                    expect = parse_env(check.get("expect"))
                    if expect and expect not in body:
                        return False, "HTTP body mismatch"
                    return True, "OK"
                return False, f"HTTP status {response.status}"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("HTTP check failed: %s", e)
            return False, f"HTTP check failed: {e}"

    elif ctype == "http3":
        expect = parse_env(check.get("expect"))
        exe, err = ensure_executable("curl")
        if not exe:
            return False, err
        try:
            res = subprocess.run(
                [exe, "-s", "--http3", "--", target],
                capture_output=True,
                text=True,
                timeout=10,
            )
            if res.returncode != 0:
                return False, f"HTTP/3 Curl failed: {res.stderr[:100]}"
            if expect and expect not in res.stdout:
                return False, "HTTP/3 body mismatch"
            return True, "OK"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("HTTP3 check failed: %s", e)
            return False, f"HTTP/3 check failed: {e}"

    elif ctype == "tcp":
        port = int(parse_env(check.get("port", 80)))
        send_payload = parse_env(check.get("send"))
        send_hex = parse_env(check.get("send_hex"))
        expect = parse_env(check.get("expect"))
        try:
            with socket.create_connection((target, port), timeout=2) as s:
                if send_hex:
                    s.sendall(binascii.unhexlify(send_hex))
                elif send_payload:
                    s.sendall(
                        send_payload.encode("utf-8")
                        .decode("unicode_escape")
                        .encode("utf-8")
                    )

                if expect:
                    response = s.recv(1024)
                    if expect.encode("utf-8") not in response:
                        return False, "TCP payload mismatch"
            return True, "OK"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("TCP check failed: %s", e)
            return False, f"TCP connection failed: {e}"

    elif ctype == "udp":
        port = int(parse_env(check.get("port", 80)))
        send_payload = parse_env(check.get("send"))
        send_hex = parse_env(check.get("send_hex"))
        expect = parse_env(check.get("expect"))
        try:
            with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as s:
                s.settimeout(2)
                if send_hex:
                    s.sendto(binascii.unhexlify(send_hex), (target, port))
                elif send_payload:
                    s.sendto(
                        send_payload.encode("utf-8")
                        .decode("unicode_escape")
                        .encode("utf-8"),
                        (target, port),
                    )
                else:
                    return False, "UDP check requires a payload to send"

                if expect:
                    response, _ = s.recvfrom(4096)
                    if expect.encode("utf-8") not in response:
                        return False, "UDP payload mismatch"
            return True, "OK"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("UDP check failed: %s", e)
            return False, f"UDP connection failed: {e}"

    elif ctype == "redis":
        port = int(parse_env(check.get("port", 6379)))
        password = parse_env(check.get("password", ""))
        try:
            r = redis_lib.Redis(
                host=target, port=port, password=password or None, socket_timeout=2
            )
            if r.ping():
                return True, "OK"
            return False, "Redis PING returned False"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("Redis check failed: %s", e)
            return False, f"Redis connection failed: {e}"

    elif ctype == "rabbitmq":
        port = int(parse_env(check.get("port", 5672)))
        try:
            with socket.create_connection((target, port), timeout=2) as s:
                s.sendall(b"AMQP\x00\x00\x09\x01")
                res = s.recv(1024)
                if len(res) > 0:
                    return True, "OK"
                return False, "RabbitMQ handshake mismatch"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("RabbitMQ check failed: %s", e)
            return False, f"RabbitMQ connection failed: {e}"

    elif ctype == "xmlrpc":
        method = parse_env(check.get("rpc_method", ""))
        params_str = parse_env(check.get("rpc_params", "[]"))
        expect = parse_env(check.get("expect"))
        try:
            params = json.loads(params_str) if params_str else []
            proxy = xmlrpc.client.ServerProxy(target)
            if method.startswith("_"):
                return False, f"Illegal RPC method: {method}"
            res = getattr(proxy, method)(*params)
            if expect and expect not in str(res):
                return False, "XML-RPC output mismatch"
            return True, "OK"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("XMLRPC check failed: %s", e)
            return False, f"XML-RPC check failed: {e}"

    elif ctype == "jsonrpc":
        method = parse_env(check.get("rpc_method", ""))
        params_str = parse_env(check.get("rpc_params", "{}"))
        expect = parse_env(check.get("expect"))
        try:
            params = json.loads(params_str) if params_str else {}
            payload = json.dumps(
                {"jsonrpc": "2.0", "method": method, "params": params, "id": 1}
            ).encode("utf-8")
            req = urllib.request.Request(
                target,
                data=payload,
                headers={"Content-Type": "application/json"},
                method="POST",
            )
            with urllib.request.urlopen(req, timeout=5) as response:
                body = response.read().decode("utf-8")
                if expect and expect not in body:
                    return False, "JSON-RPC output mismatch"
            return True, "OK"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("JSONRPC check failed: %s", e)
            return False, f"JSON-RPC check failed: {e}"

    elif ctype == "postgres" or ctype == "anomaly":
        port = int(parse_env(check.get("port", 5432)))
        dbname = parse_env(check.get("dbname", "odoo"))
        user = parse_env(check.get("user", "odoo"))
        password = parse_env(check.get("password", ""))
        query = (
            parse_env(check.get("query", "SELECT 1;"))
            if ctype == "anomaly"
            else "SELECT 1;"
        )

        conn = None
        try:
            conn = psycopg2.connect(
                host=target,
                port=port,
                dbname=dbname,
                user=user,
                password=password,
                connect_timeout=2,
            )
            with conn.cursor() as cur:
                cur.execute(query)
                val = cur.fetchone()[0]

            if ctype == "anomaly":
                critical_min = int(check.get("critical", 0))
                if val < critical_min:
                    return (
                        False,
                        f"Anomaly Threshold Breached: {val} < {critical_min}",
                    )
            return True, "OK"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("Postgres check failed: %s", e)
            return False, f"PostgreSQL/Anomaly check failed: {e}"
        finally:
            if conn:
                conn.close()

    elif ctype == "ssl":
        try:
            port = int(parse_env(check.get("port", 443)))
            ctx = ssl.create_default_context()
            with socket.create_connection((target, port), timeout=5) as sock:
                with ctx.wrap_socket(sock, server_hostname=target) as ssock:
                    cert = ssock.getpeercert()
                    expire_date = datetime.datetime.strptime(
                        cert["notAfter"], "%b %d %H:%M:%S %Y %Z"
                    )
                    days_left = (expire_date - datetime.datetime.utcnow()).days
                    critical_days = int(check.get("critical", 14))
                    if days_left <= critical_days:
                        return False, f"SSL Cert expires in {days_left} days"
            return True, "OK"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("SSL check failed: %s", e)
            return False, f"SSL check failed: {e}"

    elif ctype == "synthetic":
        script = parse_env(check.get("script", ""))
        if not script:
            return False, "Synthetic script path missing"
        try:
            res = subprocess.run(
                shlex.split(script),
                shell=False,
                capture_output=True,
                text=True,
                timeout=60,
            )
            if res.returncode != 0:
                message = f"Synthetic failure (Code {res.returncode}): {res.stderr[:100]}"
                # Graduated-severity extension, 2026-09-22: a synthetic script that wants finer
                # control than the flat "high" every other failing check gets (e.g. an
                # expiring-credential check whose urgency genuinely grows as the deadline
                # approaches) can print a bare "SEVERITY:<low|medium|high|critical>" line to
                # stderr, alongside its usual failure message (stderr, not stdout, matching
                # where every synthetic script's own diagnostic output already goes -- see
                # check_github_pat_expiry.py). Searched against the FULL res.stderr, not the
                # truncated 100-char slice used for the display message above, so the marker is
                # found regardless of how long the rest of the message is. Encoded as a
                # "[SEVERITY:xxx]" prefix on the returned message so this function's own return
                # shape (a 2-tuple) doesn't have to change for every other check type -- see
                # extract_severity_prefix()'s own docstring for the other half. A script that
                # never prints this (every synthetic check written before this existed,
                # including check_cloudflare_token_expiry.py) is completely unaffected: no
                # marker means the default "high" at the report() call site, unchanged.
                severity_match = re.search(
                    r"^SEVERITY:(low|medium|high|critical)$", res.stderr, re.MULTILINE | re.IGNORECASE
                )
                if severity_match:
                    message = f"[SEVERITY:{severity_match.group(1).lower()}] {message}"
                return False, message
            return True, "OK"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("Synthetic check failed: %s", e)
            return False, f"Synthetic execution error: {e}"

    elif ctype == "certbot":
        headers = {"User-Agent": "Pager-Daemon/1.0"}
        try:
            req = urllib.request.Request(
                "https://acme-v02.api.letsencrypt.org/directory", headers=headers
            )
            with urllib.request.urlopen(req, timeout=10) as response:
                if response.status != 200:
                    return False, f"Let's Encrypt API unreachable ({response.status})"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("Certbot check failed: %s", e)
            return False, f"Let's Encrypt API unreachable: {e}"

        domains = parse_env(check.get("target", ""))
        if domains and domains != "auto":
            try:
                req = urllib.request.Request("https://api.ipify.org", headers=headers)
                with urllib.request.urlopen(req, timeout=10) as ip_resp:
                    my_ip = ip_resp.read().decode("utf-8").strip()

                domain_list = [d.strip() for d in domains.split(",") if d.strip()]
                for d in domain_list:
                    try:
                        resolved = socket.gethostbyname(d)
                        if resolved != my_ip:
                            return (
                                False,
                                f"Domain {d} resolves to {resolved}, expected our IP {my_ip}.",
                            )
                    except socket.gaierror:
                        return False, f"Domain {d} failed DNS resolution."
            except (
                ConnectionError,
                socket.timeout,
                Exception,
            ) as e:  # audit-ignore-catch-all
                logger.warning(f"Could not verify public IP for domain matching: {e}")

        exe, _ = ensure_executable("certbot")
        if exe:
            try:
                res = subprocess.run(
                    [exe, "renew", "--dry-run"],
                    capture_output=True,
                    text=True,
                    timeout=300,
                )
                if res.returncode != 0:
                    failed = [
                        line.strip()
                        for line in (res.stdout + "\n" + res.stderr).split("\n")
                        if "Failed to renew" in line or "Challenge failed" in line
                    ]
                    err_msg = "Certbot dry-run failed!"
                    if failed:
                        err_msg += " " + " | ".join(failed[:2])
                    return False, err_msg
            except subprocess.TimeoutExpired:
                return False, "Certbot dry-run timed out."
            except (
                ConnectionError,
                socket.timeout,
                Exception,
            ) as e:  # audit-ignore-catch-all
                logger.warning("Certbot renew check failed: %s", e)
                return False, f"Certbot execution error: {e}"

        return True, "OK"

    elif ctype == "pg_dump":
        port = str(parse_env(check.get("port", 5432)))
        dbname = parse_env(check.get("dbname", "odoo"))
        user = parse_env(check.get("user", "odoo"))
        password = parse_env(check.get("password", ""))
        env = os.environ.copy()
        if password:
            env["PGPASSWORD"] = password
        exe, err = ensure_executable("pg_dump")
        if not exe:
            return False, err
        try:
            res = subprocess.run(
                [
                    exe,
                    "-s",
                    "-U",
                    user,
                    "-h",
                    target or "postgres",
                    "-p",
                    port,
                    dbname,
                ],
                capture_output=True,
                text=True,
                env=env,
                timeout=30,
                shell=False,
            )
            if res.returncode != 0:
                return False, f"pg_dump pre-flight failed: {res.stderr[:100]}"
            return True, "OK"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("pg_dump check failed: %s", e)
            return False, f"pg_dump execution error: {e}"

    elif ctype == "nginx":
        exe, err = ensure_executable("nginx")
        if not exe:
            return False, err
        try:
            res = subprocess.run(
                [exe, "-t"], capture_output=True, text=True, timeout=15
            )
            if res.returncode != 0:
                return False, f"Nginx config error: {res.stderr[:100]}"
            return True, "OK"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("Nginx check failed: %s", e)
            return False, f"Nginx execution error: {e}"

    elif ctype == "logrotate":
        exe, err = ensure_executable("logrotate")
        if not exe:
            return False, err
        conf = target or "/etc/logrotate.conf"
        try:
            res = subprocess.run(
                [exe, "-d", conf], capture_output=True, text=True, timeout=30
            )
            if res.returncode != 0:
                return False, f"Logrotate dry-run failed: {res.stderr[:100]}"
            return True, "OK"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("logrotate check failed: %s", e)
            return False, f"Logrotate execution error: {e}"

    elif ctype == "cloudflared":
        if not target:
            return False, "Cloudflared requires target (Tunnel ID or Name)"
        exe, err = ensure_executable("cloudflared")
        if not exe:
            return False, err
        try:
            res = subprocess.run(
                [exe, "tunnel", "info", target],
                capture_output=True,
                text=True,
                timeout=15,
            )
            if res.returncode != 0:
                return False, f"Cloudflared tunnel info failed: {res.stderr[:100]}"
            return True, "OK"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("cloudflared check failed: %s", e)
            return False, f"Cloudflared execution error: {e}"

    elif ctype == "smtp_dryrun":
        port = int(parse_env(check.get("port", 587)))
        user = parse_env(check.get("user", ""))
        password = parse_env(check.get("password", ""))
        if not target:
            return False, "SMTP dry-run requires target host"
        try:
            with smtplib.SMTP(target, port, timeout=10) as server:
                server.ehlo()
                if port in (587, 465) or server.has_extn("STARTTLS"):
                    server.starttls()
                    server.ehlo()
                if user and password:
                    server.login(user, password)
            return True, "OK"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("SMTP dryrun failed: %s", e)
            return False, f"SMTP dry-run failed: {e}"

    elif ctype == "icmp":
        if not target:
            return False, "ICMP requires target"
        exe, err = ensure_executable("ping")
        if not exe:
            return False, err
        try:
            res = subprocess.run(
                [exe, "-c", "3", "-W", "2", target],
                capture_output=True,
                text=True,
                timeout=10,
            )
            if res.returncode != 0:
                return (
                    False,
                    f"ICMP ping failed: {res.stderr.strip() or res.stdout.strip()[:100]}",
                )
            return True, "OK"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("ICMP check failed: %s", e)
            return False, f"ICMP execution error: {e}"

    elif ctype == "docker":
        if not target:
            return False, "Docker check requires target container name"
        exe, err = ensure_executable("docker")
        if not exe:
            return False, err
        try:
            res = subprocess.run(
                [exe, "inspect", "-f", "{{.State.Running}}", target],
                capture_output=True,
                text=True,
                timeout=10,
            )
            if res.returncode != 0:
                return False, f"Docker inspect failed: {res.stderr.strip()[:100]}"
            if res.stdout.strip().lower() != "true":
                return False, f"Docker container {target} is not running"
            return True, "OK"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("Docker check failed: %s", e)
            return False, f"Docker execution error: {e}"

    elif ctype == "memcached":
        port = int(parse_env(check.get("port", 11211)))
        if not target:
            return False, "Memcached requires target"
        try:
            with socket.create_connection((target, port), timeout=2) as s:
                s.sendall(b"stats\r\n")
                res = s.recv(1024)
                if b"STAT " not in res:
                    return False, "Memcached stats mismatch"
            return True, "OK"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("Memcached check failed: %s", e)
            return False, f"Memcached connection failed: {e}"

    elif ctype == "ssh":
        port = int(parse_env(check.get("port", 22)))
        if not target:
            return False, "SSH requires target"
        try:
            with socket.create_connection((target, port), timeout=5) as s:
                res = s.recv(1024)
                if not res.startswith(b"SSH-"):
                    return False, "SSH protocol mismatch"
            return True, "OK"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("SSH check failed: %s", e)
            return False, f"SSH connection failed: {e}"

    elif ctype == "heartbeat":
        chk_uuid = parse_env(check.get("uuid"))
        if not client:
            return False, "Client not provided for heartbeat"
        try:
            res = client.execute(
                "pager.check",
                "check_heartbeat_rpc",
                hb_uuid=chk_uuid,
                interval=int(check.get("interval", 60)),
            )
            if res:
                return True, "OK"
            return False, "Heartbeat missing"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("Heartbeat check failed: %s", e)
            return False, f"Heartbeat check failed: {e}"

    elif ctype == "smart":
        if not target:
            return False, "SMART check requires target device (e.g. /dev/sda)"

        spool_file = "/var/log/pager_smart_spool.json"
        if not os.path.exists(spool_file):
            return (
                False,
                "SMART spool file not found. Is the pager-smart-spooler.timer running?",
            )

        mtime = os.path.getmtime(spool_file)
        if time.time() - mtime > 1800:
            return (
                False,
                "SMART spool file is stale (>30 mins old). Root spooler daemon may have crashed.",
            )

        try:
            with open(spool_file, "r") as f:
                smart_data = json.load(f)

            if target not in smart_data:
                return False, f"Device {target} not found in SMART spool data."

            dev_data = smart_data[target]
            smart_status = dev_data.get("smart_status", {})
            passed = smart_status.get("passed", False)

            if not passed:
                return (
                    False,
                    f"SMART health check FAILED for {target}. Impending disk failure.",
                )

            return True, "OK"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("SMART check failed: %s", e)
            return False, f"SMART spool read error: {e}"

    elif ctype in ("playwright", "bash", "executable"):
        spool_file = "/var/log/pager_synthetic_spool.json"
        if not os.path.exists(spool_file):
            return (
                False,
                "Synthetic spool file missing. Is pager-synthetic-spooler.service running?",
            )

        mtime = os.path.getmtime(spool_file)
        if time.time() - mtime > int(check.get("interval", 60)) * 3 + 300:
            return (
                False,
                "Synthetic spool file is stale. Spooler daemon may have crashed.",
            )

        try:
            with open(spool_file, "r") as f:
                data = json.load(f)

            name = check.get("name")
            if name not in data:
                return False, f"Check '{name}' not found in synthetic spool."

            res = data[name]
            if not res.get("success"):
                err = str(res.get("error", "Unknown error"))
                return False, f"Execution Failed: {err[:200]}"
            return True, "OK"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("Synthetic script check failed: %s", e)
            return False, f"Synthetic spool read error: {e}"

    elif ctype == "file_absent":
        if not target:
            return False, "Target required"
        if os.path.exists(target):
            return False, f"File {target} exists"
        return True, "OK"

    elif ctype == "systemd":
        if not target:
            return (
                False,
                "Systemd check requires target (use '*' for all failed services)",
            )
        exe, err = ensure_executable("systemctl")
        if not exe:
            return False, err
        try:
            ignored = [
                s.strip()
                for s in parse_env(check.get("ignored_services", "")).split(",")
                if s.strip()
            ]
            if target == "*":
                res = subprocess.run(
                    [exe, "list-units", "--state=failed", "--no-legend", "--plain"],
                    capture_output=True,
                    text=True,
                    timeout=10,
                )
                if res.returncode != 0:
                    return False, f"Systemctl list-units failed: {res.stderr.strip()}"

                failed_svcs = []
                for line in res.stdout.strip().split("\n"):
                    if not line.strip():
                        continue
                    parts = line.split()
                    if parts:
                        svc_name = parts[0]
                        if svc_name not in ignored:
                            failed_svcs.append(svc_name)

                if failed_svcs:
                    return (
                        False,
                        f"Failed systemd services detected: {', '.join(failed_svcs)}",
                    )
                return True, "OK (0 failed services)"
            else:
                svcs = [s.strip() for s in target.split(",") if s.strip()]
                failed_svcs = []
                for svc in svcs:
                    res = subprocess.run(
                        [exe, "is-active", svc],
                        capture_output=True,
                        text=True,
                        timeout=10,
                    )
                    if res.returncode != 0 or res.stdout.strip() != "active":
                        failed_svcs.append(f"{svc} ({res.stdout.strip()})")

                if failed_svcs:
                    return (
                        False,
                        f"Systemd services not active: {', '.join(failed_svcs)}",
                    )
                return True, "OK"
        except (
            ConnectionError,
            socket.timeout,
            Exception,
        ) as e:  # audit-ignore-catch-all
            logger.warning("Systemd check failed: %s", e)
            return False, f"Systemd execution error: {e}"

    return False, "Unknown check type"


# [@ANCHOR: pager_duty:polling_thread]
def polling_thread(client, check):
    name = check.get("name", "Unknown")
    check_id = check.get("id")
    key = check_key(check)
    THREAD_NAMES[key] = name
    website_id = check.get("website_id")
    interval = int(check.get("interval", 60))
    grace = int(check.get("grace", 0))
    thread_start_time = time.time()

    THREAD_TIMEOUTS[key] = max(300, interval * 3)
    logger.info(
        f"Starting polling thread for [{name}] every {interval}s (Grace: {grace}s)"
    )

    THREAD_HEARTBEATS[key] = time.time()
    success, msg = execute_check(check, client)
    clean_loops = 1 if success else 0
    if not success:
        FAILING_CHECKS.add(key)
        FAILING_NAMES.add(name)
        if time.time() - thread_start_time < grace:
            logger.info(
                f"[{name}] Startup grace period active. Suppressing failure: {msg}"
            )
        else:
            severity, clean_msg = extract_severity_prefix(msg)
            report(client, name, clean_msg, severity, website_id=website_id)
            remedy = check.get("remediate")
            if clean_loops > 0 and remedy and os.path.exists(remedy):
                logger.info(f"[{name}] Triggering auto-remediation script: {remedy}")
                try:
                    subprocess.Popen([remedy], shell=False)
                except (
                    ConnectionError,
                    socket.timeout,
                    Exception,
                ) as e:  # audit-ignore-catch-all
                    logger.error(f"Remediation failed: {e}")
    else:
        FAILING_CHECKS.discard(key)
        FAILING_NAMES.discard(name)

    jitter = secrets.SystemRandom().uniform(0, interval)
    logger.info(f"[{name}] Applying startup jitter: sleeping for {jitter:.1f}s")
    time.sleep(jitter)  # audit-ignore-sleep

    while True:
        THREAD_HEARTBEATS[key] = time.time()
        parent = check.get("parent")

        if is_in_maintenance(check):
            time.sleep(interval)  # audit-ignore-sleep
            continue

        if parent_suppresses(check):
            logger.debug(f"[{name}] Suppressed due to parent '{parent}' failure.")
            time.sleep(interval)  # audit-ignore-sleep
            continue

        success, msg = execute_check(check, client)

        # Update status in Odoo
        if client and check_id:
            try:
                status = "passing" if success else "failing"
                now = datetime.datetime.utcnow().strftime("%Y-%m-%d %H:%M:%S")
                client.execute(
                    "pager.check",
                    "write",
                    ids=[check_id],
                    vals={"status": status, "last_run": now},
                )
            # Real bug, found by bug-hunt review (tier 1): this used to
            # only catch (OSError, xmlrpc.client.Fault,
            # xmlrpc.client.ProtocolError) -- leftover exception types from
            # an old XML-RPC-based client. `client` here is the JSON-2
            # OdooClient defined at the top of this module, whose
            # `execute()` raises a bare `Exception` on an HTTPError (e.g. a
            # real 401 from a bad ODOO_PASSWORD) and never raises any
            # xmlrpc.client type at all. A bare Exception wasn't caught by
            # the old tuple, so it propagated out of this thread's own
            # `while True:` loop and silently killed the whole
            # polling_thread for this check -- recoverable only once the
            # THREAD_HEARTBEATS watchdog noticed the stalled heartbeat
            # (up to `max(300, interval*3)` seconds later) and force-
            # restarted the entire daemon. Matching the broad
            # "audit-ignore-catch-all" pattern already used for every other
            # client.execute() call site in this same file.
            except (
                ConnectionError,
                socket.timeout,
                Exception,
            ) as e:  # audit-ignore-catch-all
                logger.warning(f"[{name}] Failed to update status in Odoo: {e}")

        if not success:
            FAILING_CHECKS.add(key)
            FAILING_NAMES.add(name)
            if time.time() - thread_start_time < grace:
                logger.info(
                    f"[{name}] Startup grace period active. Suppressing failure: {msg}"
                )
            else:
                severity, clean_msg = extract_severity_prefix(msg)
                report(client, name, clean_msg, severity, website_id=website_id)
                remedy = check.get("remediate")
                if clean_loops > 0 and remedy and os.path.exists(remedy):
                    logger.info(
                        f"[{name}] Triggering auto-remediation script: {remedy}"
                    )
                    try:
                        subprocess.Popen([remedy], shell=False)
                    except (
                        ConnectionError,
                        socket.timeout,
                        Exception,
                    ) as e:  # audit-ignore-catch-all
                        logger.error(f"Remediation failed: {e}")
            clean_loops = 0
        else:
            FAILING_CHECKS.discard(key)
            FAILING_NAMES.discard(name)
            clean_loops += 1
            if clean_loops == 3:
                auto_resolve(client, name, website_id=website_id)
        time.sleep(interval)  # audit-ignore-sleep


# [@ANCHOR: pager_duty:log_tail_thread]
def log_tail_thread(client, check):
    name = check.get("name", "Log Monitor")
    key = check_key(check) if check.get("id") else name
    THREAD_NAMES[key] = name
    website_id = check.get("website_id")
    filepath = parse_env(check.get("target", ""))
    regex_str = parse_env(check.get("regex", ""))
    grace = int(check.get("grace", 0))
    thread_start_time = time.time()

    THREAD_TIMEOUTS[key] = 120
    logger.info(
        f"Starting log tail thread for [{name}] on {filepath} (Grace: {grace}s)"
    )

    cur_inode = None
    f = None
    while True:
        THREAD_HEARTBEATS[key] = time.time()
        try:
            stat_obj = os.stat(filepath)
            new_inode = stat_obj.st_ino
            if cur_inode != new_inode:
                if f:
                    f.close()
                f = open(filepath, "r")
                if cur_inode is None:
                    f.seek(0, 2)
                else:
                    f.seek(0, 0)
                cur_inode = new_inode
                logger.info(f"Tailing log file {filepath} (inode: {cur_inode})")
            if f:
                line = f.readline()
                if not line:
                    time.sleep(0.5)  # audit-ignore-sleep
                    continue
                if regex_str and re.search(regex_str, line, re.IGNORECASE):
                    if time.time() - thread_start_time < grace:
                        logger.info(
                            f"[{name}] Suppressed log alert during grace period."
                        )
                    else:
                        report(
                            client,
                            name,
                            line.strip(),
                            "critical",
                            website_id=website_id,
                        )
            else:
                time.sleep(1)  # audit-ignore-sleep
        # Widened from FileNotFoundError alone (bug-hunt review, tier 1):
        # a rotated-away log file can also raise PermissionError or other
        # OSError subclasses during os.stat()/open() (e.g. a logrotate
        # permissions hiccup), which used to be uncaught here and would
        # kill this thread outright instead of retrying -- matching
        # pager_log_analyzer.py's own tail_file(), which already catches
        # the full OSError for this identical inode-rotation loop.
        except OSError:
            time.sleep(5)  # audit-ignore-sleep
            continue


# [@ANCHOR: pager_duty:daemon_resolve_config_path]
def resolve_config_path(environ=None):
    """The configuration file this daemon reads, resolved by the same rule pager_check.PagerCheck._get_config_path writes by.

    The writer prefers the system configuration directory (the `pager_duty.config_dir` parameter, default /opt/hams/etc) when
    the Odoo worker can write there, and falls back to the file next to this daemon otherwise. The daemon used to read only the
    file next to itself, so as soon as the system directory became writable every "Push to JSON" wrote a file nothing read and
    the administrator was told "Export Successful". Order here: PAGER_CONFIG_PATH (an explicit file), then pager_config.json in
    PAGER_CONFIG_DIR (default /opt/hams/etc) if it exists, then the file next to this daemon."""
    environ = os.environ if environ is None else environ
    explicit = environ.get("PAGER_CONFIG_PATH")
    if explicit:
        return explicit
    system_path = os.path.join(environ.get("PAGER_CONFIG_DIR", "/opt/hams/etc"), "pager_config.json")
    if os.path.exists(system_path):
        return system_path
    return os.path.join(os.path.dirname(__file__), "pager_config.json")


if __name__ == "__main__":
    # [@ANCHOR: daemon_main_loop]
    config_path = resolve_config_path()
    logger.info(f"Reading configuration from {config_path}")

    if not os.path.exists(config_path):
        msg = f"Configuration file not found at {config_path}. Halting."
        logger.critical(msg)
        fallback_notify("Daemon Boot", msg, "critical")
        sys.exit(1)

    try:
        with open(config_path, "r", encoding="utf-8") as f:
            config = json.load(f)
    except (ConnectionError, socket.timeout, Exception) as e:  # audit-ignore-catch-all
        msg = f"FATAL: Failed to parse {config_path} as valid JSON: {e}"
        logger.critical(msg)
        fallback_notify("Daemon Boot", msg, "critical")
        sys.exit(1)

    client = get_odoo_client(logger, config)
    if not client:
        logger.critical("Failed to connect to Odoo JSON-2. Halting.")
        sys.exit(1)

    checks = config.get("checks", [])
    logger.info(f"Loaded {len(checks)} checks from configuration.")

    verify_and_install_dependencies(client, checks)

    # Bug-hunt fix (2026-09-13): this pool submits one infinite-loop task
    # per check PLUS 2 always-on proxy threads (log_anomaly_proxy,
    # log_search_proxy, submitted below before the per-check loop) --
    # len(checks) + 2 tasks total, not len(checks) + 1. Since every task
    # is an infinite loop that never returns its worker to the pool, the
    # old undersized pool permanently starved exactly one submitted task
    # (always the LAST check in the list, since the 2 proxies are
    # submitted first and claim 2 of the pool's threads before the
    # per-check loop even starts) -- that check's own thread would never
    # run at all, silently, for the life of the daemon. Worse, the
    # THREAD_HEARTBEATS watchdog couldn't catch it either: a thread that
    # never started never writes a heartbeat key, so it never appears in
    # THREAD_HEARTBEATS.items() to be timed out -- the exact same
    # "invisible to the watchdog" gap main_loop.md's own claim already
    # documented and fixed for the 2 proxy threads themselves, one layer
    # up.
    executor = concurrent.futures.ThreadPoolExecutor(
        max_workers=max(1, len(checks) + 2)
    )
    futures = []

    def log_anomaly_proxy(cl):
        r = redis_lib.Redis(
            host=os.environ.get("REDIS_HOST") or "redis",
            port=int(os.environ.get("REDIS_PORT") or "6379"),
            db=0,
            username=REDIS_USERNAME,
            password=REDIS_PASS,
            decode_responses=True,
        )
        # Bug-hunt fix (2026-09-13): this thread was never covered by the
        # THREAD_HEARTBEATS watchdog below (main_loop.md's own claim
        # flagged this) -- a Redis connection that accepts the TCP
        # handshake but never responds would hang here with blpop's own
        # 5s timeout never firing (some intermediary proxy), and the
        # watchdog would never notice since this thread never appeared as
        # a key in THREAD_HEARTBEATS at all. Timeout matches
        # polling_thread's own established floor (max(300, interval*3))
        # rather than a tighter value: the watchdog's response is
        # os._exit(1), a hard, ungraceful kill of the WHOLE daemon
        # (every check thread along with it, no cleanup) -- a per-
        # iteration Odoo JSON-2 RPC via report() can legitimately take up
        # to its own 5s urlopen timeout, so a tighter bound buys no real
        # protection against a genuine indefinite hang (still caught) but
        # adds real risk of a false-positive full-daemon kill on a
        # slow-but-alive RPC burst.
        THREAD_TIMEOUTS["log_anomaly_proxy"] = 300
        while True:
            THREAD_HEARTBEATS["log_anomaly_proxy"] = time.time()
            try:
                res = r.blpop("pager_log_anomalies", timeout=5)
                if res:
                    _, data = res
                    payload = json.loads(data)
                    report(
                        cl,
                        payload["source"],
                        payload["description"],
                        payload["severity"],
                        website_id=payload.get("website_id"),
                    )
            except (
                ConnectionError,
                socket.timeout,
                Exception,
            ) as e:  # audit-ignore-catch-all
                logger.warning("Anomaly proxy loop error: %s", e)
                time.sleep(1)  # audit-ignore-sleep

    def log_search_proxy(cl):
        r = redis_lib.Redis(
            host=os.environ.get("REDIS_HOST") or "redis",
            port=int(os.environ.get("REDIS_PORT") or "6379"),
            db=0,
            username=REDIS_USERNAME,
            password=REDIS_PASS,
            decode_responses=True,
        )
        # Bug-hunt fix (2026-09-13): same watchdog-coverage gap and fix as
        # log_anomaly_proxy above -- see its own comment, including why
        # this uses the file's own 300s floor rather than a tighter bound.
        THREAD_TIMEOUTS["log_search_proxy"] = 300
        while True:
            THREAD_HEARTBEATS["log_search_proxy"] = time.time()
            try:
                res = r.blpop("pager_log_search_res_queue", timeout=5)
                if res:
                    _, data = res
                    data_dict = json.loads(data)
                    uuid_str = data_dict.get("uuid")
                    payload = data_dict.get("payload", {})
                    state = "error" if "error" in payload else "done"
                    cl.execute(
                        "pager.log.search.job",
                        "rpc_update_state",
                        uuid=uuid_str,
                        state=state,
                        result_payload=json.dumps(payload),
                    )
            except (
                ConnectionError,
                socket.timeout,
                Exception,
            ) as e:  # audit-ignore-catch-all
                logger.warning("Search proxy loop error: %s", e)
                time.sleep(1)  # audit-ignore-sleep

    futures.append(executor.submit(log_anomaly_proxy, client))
    futures.append(executor.submit(log_search_proxy, client))

    for check in checks:
        if check.get("type") == "log":
            futures.append(executor.submit(log_tail_thread, client, check))
        else:
            futures.append(executor.submit(polling_thread, client, check))

    try:
        while True:
            time.sleep(10)  # audit-ignore-sleep
            now = time.time()
            for t_name, last_beat in list(THREAD_HEARTBEATS.items()):
                timeout = THREAD_TIMEOUTS.get(t_name, 300)
                if now - last_beat > timeout:
                    logger.critical(
                        f"WATCHDOG: Thread '{THREAD_NAMES.get(t_name, t_name)}' (key {t_name}) hung for {now - last_beat:.1f}s (Timeout: {timeout}s)! Force restarting daemon."
                    )
                    os._exit(1)
    except KeyboardInterrupt:
        logger.info("Shutting down monitoring daemon.")
