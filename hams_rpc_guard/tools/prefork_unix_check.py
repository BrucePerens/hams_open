#!/usr/bin/env python3
# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Manual check of the Unix-socket listener under Odoo's prefork server (`--workers 2`), on a throwaway instance:
its own PostgreSQL cluster in a scratch directory (no shared cluster is touched), a scratch HTTP port, a scratch socket.

    prefork_unix_check.py prepare DIR --addons-path PATHS [--http-port 18969] [--pg-port 15499]
        initdb + start the scratch cluster (pg_ctl, socket only), write DIR/odoo.conf, install hams_rpc_guard into the
        scratch database, set rpc_transport=unix and unix_peers=<this user>; prints the command that runs Odoo.
    (run the printed command under a process supervisor, for example hams_shared/tools/agent_procs.py run NAME -- ...)
    prefork_unix_check.py check DIR
        the prefork checks below; exit 0 when all pass, 1 otherwise.
    prefork_unix_check.py teardown DIR
        stop the scratch cluster (pg_ctl stop -m fast) and remove DIR.

Checks: the socket exists with mode 0660; the master and every HTTP worker hold the listening socket, the cron worker and
the gevent process do not; the endpoints answer on the socket (legacy `version` 200, JSON-2 with a bad key 401 from
Odoo's own authentication) and are refused on TCP (404) while the web client's `/web/login` still answers on TCP; a
non-RPC path answers 404 on the socket; idle connections that hold every worker are dropped after the 2-second socket
timeout and requests are served again; repeated requests are served (spread over the workers); no worker was killed.
"""
import argparse
import getpass
import ipaddress
import os
import re
import shutil
import socket
import stat
import subprocess
import sys
import time

DB = "hrg_prefork_check"
PG_BIN_DIRS = ("/usr/lib/postgresql/18/bin", "/usr/lib/postgresql/17/bin")
VERSION_CALL = b'<?xml version="1.0"?><methodCall><methodName>version</methodName><params/></methodCall>'


def _pg(tool):
    for directory in PG_BIN_DIRS:
        if os.path.exists(os.path.join(directory, tool)):
            return os.path.join(directory, tool)
    sys.exit("PostgreSQL tool %s not found in %s" % (tool, PG_BIN_DIRS))


def _conf(directory):
    return os.path.join(directory, "odoo.conf")


def prepare(args):
    directory = os.path.abspath(args.dir)
    os.makedirs(directory, exist_ok=False)
    user = getpass.getuser()
    pgdata = os.path.join(directory, "pg")
    subprocess.run([_pg("initdb"), "-D", pgdata, "-A", "trust", "-U", user], check=True, stdout=subprocess.DEVNULL)
    subprocess.run([_pg("pg_ctl"), "-D", pgdata, "-l", os.path.join(directory, "pg.log"), "-w", "-o",
                    "-k %s -p %d -c listen_addresses=''" % (directory, args.pg_port), "start"], check=True)
    with open(_conf(directory), "w") as handle:
        handle.write("\n".join([
            "[options]",
            "db_host = %s" % directory,
            "db_port = %d" % args.pg_port,
            "db_user = %s" % user,
            "db_name = %s" % DB,
            "dbfilter = ^%s$" % DB,
            "addons_path = %s" % args.addons_path,
            "http_interface = %s" % args.http_interface,
            "http_port = %d" % args.http_port,
            "gevent_port = %d" % (args.http_port + 3),
            "workers = 2",
            "max_cron_threads = 1",
            "limit_time_real = 30",
            "server_wide_modules = base,rpc,web,hams_rpc_guard",
            "hams_rpc_guard_socket = %s" % os.path.join(directory, "rpc.sock"),
            "hams_rpc_guard_socket_mode = 0660",
            "logfile = %s" % os.path.join(directory, "odoo.log"),
            "",
        ]))
    subprocess.run(["odoo", "-c", _conf(directory), "-d", DB, "-i", "hams_rpc_guard", "--without-demo=all",
                    "--stop-after-init", "--workers=0", "--load=base,rpc,web"], check=True)
    psql = [_pg("psql"), "-h", directory, "-p", str(args.pg_port), "-d", DB, "-v", "ON_ERROR_STOP=1", "-c"]
    for key, value in (("hams_rpc_guard.rpc_transport", "unix"), ("hams_rpc_guard.unix_peers", user)):
        subprocess.run(psql + ["INSERT INTO ir_config_parameter (key, value) VALUES ('%s', '%s')" % (key, value)],
                       check=True, stdout=subprocess.DEVNULL)
    print("Run: odoo -c %s" % _conf(directory))


def _http(path, method, address, body=b"", headers=None, timeout=10):
    """(status, body) of one HTTP/1.0 request over a Unix socket (`address` a path) or TCP (`address` (host, port))."""
    family = socket.AF_UNIX if isinstance(address, str) else socket.AF_INET
    with socket.socket(family, socket.SOCK_STREAM) as sock:
        sock.settimeout(timeout)
        sock.connect(address)
        lines = ["%s %s HTTP/1.0" % (method, path), "Host: check", "Content-Length: %d" % len(body)]
        lines += ["%s: %s" % item for item in (headers or {}).items()]
        sock.sendall(("\r\n".join(lines) + "\r\n\r\n").encode() + body)
        data = b""
        while chunk := sock.recv(65536):
            data += chunk
    status = int(data.split(b" ", 2)[1])
    return status, data.split(b"\r\n\r\n", 1)[-1]


def _children(pid):
    found = []
    for entry in os.listdir("/proc"):
        if entry.isdigit():
            try:
                with open("/proc/%s/stat" % entry) as handle:
                    fields = handle.read().rsplit(")", 1)[1].split()
                with open("/proc/%s/cmdline" % entry, "rb") as handle:
                    cmdline = handle.read().replace(b"\0", b" ").decode(errors="replace")
            except OSError:
                continue
            if int(fields[1]) == pid:
                found.append((int(entry), cmdline))
    return found


def _socket_inode(path):
    with open("/proc/net/unix") as handle:
        for line in handle:
            parts = line.split()
            if len(parts) >= 8 and parts[7] == path and parts[5] == "01":  # state 01: listening
                return parts[6]
    return None


def _holds(pid, inode):
    try:
        return any(os.readlink("/proc/%d/fd/%s" % (pid, fd)) == "socket:[%s]" % inode
                   for fd in os.listdir("/proc/%d/fd" % pid))
    except OSError:
        return False


def check(args):
    directory = os.path.abspath(args.dir)
    conf = open(_conf(directory)).read()
    port = int(re.search(r"^http_port = (\d+)", conf, re.M).group(1))
    host = re.search(r"^http_interface = (\S+)", conf, re.M).group(1)
    path = os.path.join(directory, "rpc.sock")
    results = []

    def expect(name, ok):
        results.append(ok)
        print("%s  %s" % ("PASS" if ok else "FAIL", name))

    deadline = time.time() + 120
    while time.time() < deadline and not os.path.exists(path):
        time.sleep(1)
    expect("socket exists with mode 0660", os.path.exists(path) and stat.S_IMODE(os.stat(path).st_mode) == 0o660)
    time.sleep(5)  # workers spawned
    candidates = [int(entry) for entry in os.listdir("/proc") if entry.isdigit()
                  and _conf(directory) in _cmd(int(entry)) and "gevent" not in _cmd(int(entry))]
    master = [pid for pid in candidates if any("WorkerHTTP" in cmd for _child, cmd in _children(pid))]
    expect("one Odoo master found", len(master) == 1)
    if len(master) != 1:
        return 1
    inode = _socket_inode(path)
    children = _children(master[0])
    expect("master holds the listening socket", inode is not None and _holds(master[0], inode))
    workers = [(pid, _title(pid)) for pid, _cmd_line in children]
    print("     children: %s" % ", ".join("%d %s" % (pid, title) for pid, title in workers))
    http = [pid for pid, title in workers if "WorkerHTTP" in title]
    cron = [pid for pid, title in workers if "WorkerCron" in title]
    gevent = [pid for pid, title in workers if "gevent" in title]
    expect("two HTTP workers, each holds the socket", len(http) == 2 and all(_holds(pid, inode) for pid in http))
    expect("cron worker does not hold it", bool(cron) and not any(_holds(pid, inode) for pid in cron))
    expect("gevent process does not hold it", not any(_holds(pid, inode) for pid in gevent))
    json2_headers = {"Content-Type": "application/json", "Authorization": "Bearer not-a-real-key", "X-Odoo-Database": DB}
    status, body = _http("/xmlrpc/2/common", "POST", path, VERSION_CALL, {"Content-Type": "text/xml"})
    expect("legacy version on the socket: 200", status == 200 and b"server_version" in body)
    expect("JSON-2 bad key on the socket: 401 (reached Odoo's authentication)",
           _http("/json/2/res.users/context_get", "POST", path, b"{}", json2_headers)[0] == 401)
    expect("JSON-2 on TCP with rpc_transport=unix: 404",
           _http("/json/2/res.users/context_get", "POST", (host, port), b"{}", json2_headers)[0] == 404)
    expect("legacy on TCP with rpc_transport=unix: 404",
           _http("/xmlrpc/2/common", "POST", (host, port), VERSION_CALL, {"Content-Type": "text/xml"})[0] == 404)
    expect("/web/login still answers on TCP", _http("/web/login", "GET", (host, port))[0] == 200)
    expect("/web/login on the socket: 404", _http("/web/login", "GET", path)[0] == 404)
    idle = []
    for _ in range(4):  # more idle connections than workers
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.connect(path)
        sock.sendall(b"POST /xmlrpc/2/common HTTP/1.0\r\n")  # never finished
        idle.append(sock)
    started = time.time()
    status = _http("/xmlrpc/2/common", "POST", path, VERSION_CALL, {"Content-Type": "text/xml"}, timeout=30)[0]
    waited = time.time() - started
    expect("idle clients are dropped by the socket timeout, then served (%.1fs)" % waited, status == 200 and waited < 20)
    for sock in idle:
        sock.close()
    statuses = [_http("/xmlrpc/2/common", "POST", path, VERSION_CALL, {"Content-Type": "text/xml"})[0] for _ in range(40)]
    expect("40 requests on the socket all served", statuses == [200] * 40)
    after = [pid for pid, _cmd_line in _children(master[0]) if "WorkerHTTP" in _title(pid)]
    expect("HTTP workers unchanged (none killed by the watchdog)", sorted(after) == sorted(http))
    log = open(os.path.join(directory, "odoo.log")).read()
    expect("no timeout or traceback in the Odoo log", " timeout after " not in log and "Traceback" not in log)
    return 0 if all(results) else 1


def _cmd(pid):
    try:
        with open("/proc/%d/cmdline" % pid, "rb") as handle:
            return handle.read().replace(b"\0", b" ").decode(errors="replace").strip()
    except OSError:
        return ""


def _title(pid):
    return _cmd(pid)


def teardown(args):
    directory = os.path.abspath(args.dir)
    pgdata = os.path.join(directory, "pg")
    if os.path.exists(os.path.join(pgdata, "postmaster.pid")):
        subprocess.run([_pg("pg_ctl"), "-D", pgdata, "-m", "fast", "-w", "stop"], check=False)
    shutil.rmtree(directory, ignore_errors=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("prepare")
    p.add_argument("dir")
    p.add_argument("--addons-path", required=True)
    p.add_argument("--http-port", type=int, default=18969)
    p.add_argument("--http-interface", default=str(ipaddress.IPv4Address(0x7F000001)))
    p.add_argument("--pg-port", type=int, default=15499)
    sub.add_parser("check").add_argument("dir")
    sub.add_parser("teardown").add_argument("dir")
    args = parser.parse_args()
    return {"prepare": prepare, "check": check, "teardown": teardown}[args.command](args) or 0


if __name__ == "__main__":
    sys.exit(main())
