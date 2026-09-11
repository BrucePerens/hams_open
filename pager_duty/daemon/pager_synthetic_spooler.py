# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
import os
import sys
import time
import json
import subprocess
import hashlib
import ipaddress
import socket
import urllib.error
import urllib.request
from urllib.parse import urlparse
import tempfile
import concurrent.futures
import shlex
import logging

logger = logging.getLogger(__name__)


# Bug-hunt fix, 2026-09-11: execute_check()'s sandbox_downloads fetch below
# runs in THIS daemon's own parent process, before any of the bwrap
# sandboxing further down in this same function is ever applied -- and that
# sandboxing's own "sandbox_network_access" setting (defaulting to
# "loopback", i.e. no real network access at all) only governs the LATER
# execution step, not this download. A check config whose sandbox_downloads
# URL points at an internal/loopback/link-local target (the classic
# 169.254.169.254 cloud-metadata address, or any other internal-only
# service reachable from this daemon's own network position) would be
# fetched from here regardless of that setting -- a real SSRF primitive.
# Ported from binary_downloader/models/binary_utils.py's own
# _is_ssrf_safe_public_ip/_assert_host_is_ssrf_safe (that module's own
# 2026-09-09 bug-hunt fix for the identical shape of bug in a different
# download path) rather than reinventing the check -- this file has no
# Odoo import at all (it's a standalone daemon script), so the logic is
# duplicated in pure-stdlib form instead of imported.
def _is_ssrf_safe_public_ip(ip_obj):
    return (
        ip_obj.is_global
        and not ip_obj.is_private
        and not ip_obj.is_loopback
        and not ip_obj.is_link_local
        and not ip_obj.is_multicast
        and not ip_obj.is_reserved
        and not ip_obj.is_unspecified
    )


# [@ANCHOR: pager_duty:synthetic_spooler_ssrf_guard]
def _assert_host_is_ssrf_safe(hostname, context):
    if not hostname:
        raise ValueError(f"Security Alert: download URL for {context} has no hostname.")
    try:
        addrinfo = socket.getaddrinfo(hostname, None)
    except OSError as e:
        raise ValueError(f"Could not resolve download host for {context}: {e}")
    for info in addrinfo:
        sockaddr = info[4]
        try:
            ip_obj = ipaddress.ip_address(sockaddr[0])
        except ValueError:
            continue
        if not _is_ssrf_safe_public_ip(ip_obj):
            raise ValueError(
                f"Security Alert: download URL for {context} resolves to a "
                f"non-public address ({sockaddr[0]}). Refusing to fetch from "
                f"an internal/loopback/link-local network target."
            )


# Overridable so a test doesn't have to write into the real, hardcoded
# system path -- matching check_cloudflare_token_expiry.py's/
# pager_smart_spooler.py's own established HAMS_*_PATH override
# convention.
SPOOL_FILE = os.environ.get("HAMS_SYNTHETIC_SPOOL_PATH") or "/var/log/pager_synthetic_spool.json"  # burn-ignore-env


def execute_check(check):
    # [@ANCHOR: synthetic_i18n]
    ctype = check.get("type")
    name = check.get("name")
    interval = int(check.get("interval", 60))

    res = {"success": False, "error": ""}

    try:
        with tempfile.TemporaryDirectory() as tmpdir:
            # 1. Process Downloads and Verify Cryptographic Hashes
            downloads = check.get("sandbox_downloads", "")
            if downloads and ctype in ("bash", "executable"):
                for line in downloads.splitlines():
                    if not line.strip():
                        continue
                    parts = [p.strip() for p in line.split("|")]
                    if len(parts) == 3:
                        url, checksum, fname = parts
                        if not url.startswith(("http://", "https://")):
                            raise ValueError(f"Invalid URL scheme: {url}")
                        _assert_host_is_ssrf_safe(urlparse(url).hostname, name)
                        target_path = os.path.join(tmpdir, os.path.basename(fname))
                        req = urllib.request.Request(
                            url,
                            headers={"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/146.0.0.0 Safari/537.36"}
                        )
                        with urllib.request.urlopen(req, timeout=30) as response, open(target_path, "wb") as out_file:
                            # urllib's default opener follows HTTP redirects
                            # on its own, with nothing re-validating where a
                            # redirect actually landed -- an otherwise-safe
                            # URL could 302 to an internal target, and the
                            # bytes fetched from THAT response are exactly
                            # what gets hashed and (if it somehow matched)
                            # marked executable below. Re-check the
                            # resolved final URL's host before trusting
                            # anything read from this response.
                            final_url = getattr(response, "geturl", lambda: None)()
                            if isinstance(final_url, str) and final_url:
                                _assert_host_is_ssrf_safe(urlparse(final_url).hostname, name)
                            out_file.write(response.read())

                        hasher = hashlib.sha256()
                        with open(target_path, "rb") as f:
                            hasher.update(f.read())
                        if hasher.hexdigest() != checksum:
                            raise Exception(
                                f"Checksum mismatch for downloaded file: {fname}"
                            )
                        os.chmod(target_path, 0o755)

            # 2. Execute Framework
            if ctype == "playwright":
                script_path = os.path.join(tmpdir, "script.py")
                with open(script_path, "w") as f:
                    f.write(check.get("code_payload", ""))

                bwrap_cmd = [
                    "bwrap",
                    "--ro-bind",
                    "/",
                    "/",
                    "--dev",
                    "/dev",
                    "--proc",
                    "/proc",
                    "--tmpfs",
                    "/tmp",
                    "--unshare-user",
                    "--unshare-ipc",
                    "--unshare-pid",
                    "--unshare-uts",
                    "--unshare-cgroup-try",
                ]
                if check.get("sandbox_network_access", "loopback") == "loopback":
                    bwrap_cmd.append("--unshare-net")

                bwrap_cmd.extend(
                    [
                        "--bind",
                        tmpdir,
                        "/tmp/workspace",
                        "--chdir",
                        "/tmp/workspace",
                        "--die-with-parent",
                        "python3",
                        "script.py",
                    ]
                )
                proc = subprocess.run(
                    bwrap_cmd,
                    capture_output=True,
                    text=True,
                    timeout=interval,
                    shell=False,
                )

                if proc.returncode != 0:
                    res["error"] = proc.stderr.strip()
                else:
                    res["success"] = True

            elif ctype == "bash":
                script_path = os.path.join(tmpdir, "script.sh")
                with open(script_path, "w") as f:
                    f.write(check.get("code_payload", ""))
                os.chmod(script_path, 0o755)

                bwrap_cmd = [
                    "bwrap",
                    "--ro-bind",
                    "/",
                    "/",
                    "--dev",
                    "/dev",
                    "--proc",
                    "/proc",
                    "--tmpfs",
                    "/tmp",
                    "--unshare-user",
                    "--unshare-ipc",
                    "--unshare-pid",
                    "--unshare-uts",
                    "--unshare-cgroup-try",
                ]
                if check.get("sandbox_network_access", "loopback") == "loopback":
                    bwrap_cmd.append("--unshare-net")

                bwrap_cmd.extend(
                    [
                        "--bind",
                        tmpdir,
                        "/tmp/workspace",
                        "--chdir",
                        "/tmp/workspace",
                        "--die-with-parent",
                        "/bin/bash",
                        "script.sh",
                    ]
                )
                proc = subprocess.run(
                    bwrap_cmd,
                    capture_output=True,
                    text=True,
                    timeout=interval,
                    shell=False,
                )
                if proc.returncode != 0:
                    res["error"] = proc.stderr.strip()
                else:
                    res["success"] = True
                    res["output"] = proc.stdout.strip()

            elif ctype == "executable":
                exe_path = check.get("executable_path", "")
                exe_args = check.get("executable_args", "")

                if not exe_path.startswith("/"):
                    exe_path = f"/tmp/workspace/{exe_path}"

                bwrap_cmd = [
                    "bwrap",
                    "--ro-bind",
                    "/",
                    "/",
                    "--dev",
                    "/dev",
                    "--proc",
                    "/proc",
                    "--tmpfs",
                    "/tmp",
                    "--unshare-user",
                    "--unshare-ipc",
                    "--unshare-pid",
                    "--unshare-uts",
                    "--unshare-cgroup-try",
                ]
                if check.get("sandbox_network_access", "loopback") == "loopback":
                    bwrap_cmd.append("--unshare-net")

                bwrap_cmd.extend(
                    [
                        "--bind",
                        tmpdir,
                        "/tmp/workspace",
                        "--chdir",
                        "/tmp/workspace",
                        "--die-with-parent",
                        exe_path,
                    ]
                )

                if exe_args:
                    bwrap_cmd.extend(shlex.split(exe_args))

                proc = subprocess.run(
                    bwrap_cmd,
                    capture_output=True,
                    text=True,
                    timeout=interval,
                    shell=False,
                )
                if proc.returncode != 0:
                    res["error"] = proc.stderr.strip()
                else:
                    res["success"] = True

    except subprocess.TimeoutExpired as e:
        logger.warning("Execution timed out: %s", e)
        res["error"] = "Execution timed out"
    except (OSError, subprocess.SubprocessError) as e:
        logger.warning("Execution error: %s", e)
        res["error"] = str(e)
    # Real bug, found by bug-hunt review (tier 1): the `raise ValueError(...)`
    # (bad sandbox_downloads URL scheme) and `raise Exception(...)` (checksum
    # mismatch) a few lines above are neither OSError nor
    # subprocess.SubprocessError, so they used to escape this function
    # entirely uncaught. main() submits execute_check() per check via a
    # ThreadPoolExecutor and calls future.result() inside a try that only
    # catches concurrent.futures.CancelledError -- so one check with a bad
    # download URL or a flaky/replaced download (checksum mismatch) would
    # crash the *entire* spooler daemon's main loop, not just report that one
    # check's own `res["error"]` the way this function's return-value design
    # (and every other failure branch here) already does. Since
    # pager-synthetic-spooler.service restarts on failure and re-reads the
    # same config immediately, a single persistently-misconfigured check
    # would crash-loop the daemon and stop reporting on every OTHER
    # playwright/bash/executable check too. This broad catch restores the
    # intended per-check isolation: any failure in this function becomes
    # res["error"], never an escaped exception.
    except Exception as e:  # audit-ignore-catch-all -- deliberate per-check isolation boundary
        logger.warning("Unexpected execution error: %s", e)
        res["error"] = str(e)

    return name, res


# [@ANCHOR: pager_duty:synthetic_spooler_main]
def main():
    config_path = os.path.join(os.path.dirname(__file__), "pager_config.json")
    if not os.path.exists(config_path):
        # Real bug, found by bug-hunt review (tier 1): this branch used to
        # return 1 with no log line at all, and the `if __name__ ==
        # "__main__":` guard below called `main()` without `sys.exit(...)`,
        # so a missing config file made the process exit cleanly (code 0)
        # having logged nothing whatsoever -- under
        # pager-synthetic-spooler.service's `Restart=always`/`RestartSec=10`
        # that's a completely silent crash-loop (systemd restarts on ANY
        # exit under Restart=always, clean or not) instead of the loud,
        # visible failure a missing config deserves.
        logger.critical("pager_config.json not found at %s.", config_path)
        return 1

    try:
        with open(config_path, "r", encoding="utf-8") as f:
            config = json.load(f)
    except (OSError, json.JSONDecodeError) as e:
        logger.error("Failed to parse config: %s", e)
        return 1

    checks = [
        c
        for c in config.get("checks", [])
        if c.get("type") in ("playwright", "bash", "executable")
    ]

    last_runs = {}
    spool_data = {}

    while True:
        now = time.time()
        futures = {}
        with concurrent.futures.ThreadPoolExecutor(max_workers=5) as executor:
            for c in checks:
                name = c.get("name")
                interval = int(c.get("interval", 60))

                if now - last_runs.get(name, 0) >= interval:
                    futures[executor.submit(execute_check, c)] = name
                    last_runs[name] = now

            for future in concurrent.futures.as_completed(futures):
                try:
                    name, res = future.result()
                    spool_data[name] = res
                except concurrent.futures.CancelledError as e:
                    logger.warning("Future result extraction error: %s", e)

        if spool_data:
            fd, tmp_file = tempfile.mkstemp(dir=os.path.dirname(SPOOL_FILE))
            with os.fdopen(fd, "w") as f:
                json.dump(spool_data, f)
            os.chmod(tmp_file, 0o644)
            os.rename(tmp_file, SPOOL_FILE)

        time.sleep(5)  # audit-ignore-sleep


if __name__ == "__main__":
    # Real bug, found by bug-hunt review (tier 1): this used to call
    # main() without sys.exit(...), so main()'s own `return 1` on a
    # missing/unparseable config was silently discarded -- the process
    # always exited 0 regardless of whether it actually did anything.
    sys.exit(main())
