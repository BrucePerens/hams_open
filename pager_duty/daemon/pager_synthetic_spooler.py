# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
import os
import sys
import time
import json
import subprocess
import hashlib
import urllib.error
import urllib.request
from urllib.parse import urlparse
import tempfile
import concurrent.futures
import shlex
import logging

logger = logging.getLogger(__name__)

# Bug-hunt fix, 2026-09-14: the SSRF classification predicate + the
# resolve-and-pin fetch mechanism used to be duplicated here in pure-stdlib
# form (see the "Ported from binary_downloader" note below, kept for
# history) because this file has no Odoo import at all (it's a real,
# standalone systemd-managed script -- see pager-synthetic-spooler.service
# -- and hams_shared/tools/check_burn_list.py's own "CRITICAL DAEMON
# DECOUPLING" rule bans `import odoo`/`from odoo` in any daemon/ directory
# outright). Now imported from zero_sudo/daemon/ssrf_safe_fetch.py -- a
# plain, Odoo-free module living in a sibling addon's own `daemon/`
# directory, reached via a `sys.path` hop rather than the `odoo.addons`
# namespace (which requires the `odoo` package itself to be importable --
# confirmed NOT the case in this daemon's own real process environment) --
# instead of a second, independently-duplicated copy. See that module's own
# docstring for the real DNS-rebinding TOCTOU this closes (this file's own
# hostname check and the actual `urlopen()` connection used to be two
# separate, independently-timed DNS lookups; the fix pins the connection to
# the exact address already validated, rather than trusting a second
# lookup) and why `zero_sudo` (already a hard Odoo `depends` of both
# `pager_duty` and `binary_downloader`) is the shared home.
_ZERO_SUDO_DAEMON_DIR = os.path.normpath(
    os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "zero_sudo", "daemon")
)
if _ZERO_SUDO_DAEMON_DIR not in sys.path:
    sys.path.insert(0, _ZERO_SUDO_DAEMON_DIR)
from ssrf_safe_fetch import (  # noqa: E402
    SSRFValidationError,
    is_ssrf_safe_public_ip as _shared_is_ssrf_safe_public_ip,
    resolve_ssrf_safe_addresses as _resolve_ssrf_safe_addresses,
    urlopen_ssrf_safe as _urlopen_ssrf_safe,
)


# Ported from binary_downloader/models/binary_utils.py's own
# _is_ssrf_safe_public_ip/_assert_host_is_ssrf_safe (that module's own
# 2026-09-09 bug-hunt fix for the identical shape of bug in a different
# download path) rather than reinventing the check -- as of 2026-09-14 both
# this function and _assert_host_is_ssrf_safe below are thin wrappers
# around the shared zero_sudo.daemon.ssrf_safe_fetch implementation
# (imported above), not a fresh duplicate -- kept as real functions (not a
# bare re-export) so these anchors stay attached to a real function span
# for check_claims_freshness.py's own AST-based hashing, and every existing
# caller/test keeps working unchanged.
# [@ANCHOR: pager_duty:synthetic_spooler_is_ssrf_safe_public_ip]
# Verified by [@ANCHOR: test_is_ssrf_safe_public_ip_classifies_real_addresses_correctly]
def _is_ssrf_safe_public_ip(ip_obj):
    return _shared_is_ssrf_safe_public_ip(ip_obj)


# [@ANCHOR: pager_duty:synthetic_spooler_ssrf_guard]
def _assert_host_is_ssrf_safe(hostname, context):
    try:
        _resolve_ssrf_safe_addresses(hostname, context)
    except SSRFValidationError as e:
        raise ValueError(f"Security Alert: {e}") from e


# Overridable so a test doesn't have to write into the real, hardcoded
# system path -- matching check_cloudflare_token_expiry.py's/
# pager_smart_spooler.py's own established HAMS_*_PATH override
# convention.
SPOOL_FILE = os.environ.get("HAMS_SYNTHETIC_SPOOL_PATH") or "/var/log/pager_synthetic_spool.json"  # burn-ignore-env

# Bug-hunt fix, 2026-09-14: hard ceiling on a single sandbox_downloads fetch.
# Module-level (not a local constant) so a test can override it without a
# multi-hundred-MB fixture. A `pager.check` whose `sandbox_downloads` target
# is compromised, misconfigured, or simply serves an unexpectedly large
# response used to be read in ONE unbounded `response.read()` call straight
# into memory -- unlike binary_utils.py's own sibling download path, which at
# least streams in bounded chunks. Since `execute_check` re-runs on every
# `interval` (as short as 60s) for as long as the check exists, an
# unbounded response is a repeatable memory-exhaustion DoS against this
# daemon's own process, not a one-off.
MAX_SANDBOX_DOWNLOAD_BYTES = 200 * 1024 * 1024


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
                        try:
                            # Bug-hunt fix, 2026-09-14: _urlopen_ssrf_safe()
                            # (not plain urllib.request.urlopen()) resolves
                            # + validates + pins the real connection to the
                            # validated address on every hop (this request
                            # and any redirect it follows) -- see
                            # zero_sudo/daemon/ssrf_safe_fetch.py's own
                            # docstring for the DNS-rebinding TOCTOU this
                            # closes that the manual
                            # _assert_host_is_ssrf_safe() pre-check above,
                            # by itself, never could. The manual
                            # final-URL re-check below is kept anyway as
                            # defense in depth (and to give an immediate,
                            # specific error for the common case) --
                            # harmless now that the real connection is
                            # already pinned regardless.
                            with _urlopen_ssrf_safe(req, name, https_only=False, timeout=30) as response, open(target_path, "wb") as out_file:
                                final_url = getattr(response, "geturl", lambda: None)()
                                if isinstance(final_url, str) and final_url:
                                    _assert_host_is_ssrf_safe(urlparse(final_url).hostname, name)
                                # Bug-hunt fix, 2026-09-14: stream in bounded
                                # chunks and enforce MAX_SANDBOX_DOWNLOAD_BYTES
                                # as we go (not via a trusted, attacker-
                                # controlled Content-Length header, which a
                                # malicious/compromised server can simply omit
                                # or under-report) -- see that constant's own
                                # comment for the DoS this closes.
                                downloaded = 0
                                for chunk in iter(lambda: response.read(65536), b""):
                                    downloaded += len(chunk)
                                    if downloaded > MAX_SANDBOX_DOWNLOAD_BYTES:
                                        raise ValueError(
                                            f"sandbox_downloads target for {fname} "
                                            f"exceeded the {MAX_SANDBOX_DOWNLOAD_BYTES}"
                                            f"-byte cap; aborted mid-download."
                                        )
                                    out_file.write(chunk)
                        except SSRFValidationError as e:
                            raise ValueError(f"Security Alert: {e}") from e

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
