# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
import os
# import sys
import time
import json
import subprocess
import hashlib
import urllib.request
import tempfile
import concurrent.futures
import shlex
import logging

logger = logging.getLogger(__name__)
SPOOL_FILE = "/var/log/pager_synthetic_spool.json"


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
                        target_path = os.path.join(tmpdir, os.path.basename(fname))
                        req = urllib.request.Request(
                            url,
                            headers={"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/146.0.0.0 Safari/537.36"}
                        )
                        with urllib.request.urlopen(req, timeout=30) as response, open(target_path, "wb") as out_file:
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

    return name, res


def main():
    config_path = os.path.join(os.path.dirname(__file__), "pager_config.json")
    if not os.path.exists(config_path):
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
    main()
