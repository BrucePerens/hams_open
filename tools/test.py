#!/usr/bin/env python3
"""
Unified Pure-Python Odoo Test Runner for Hams.com
Combines test execution, integration modes, and real-time failure extraction.
Strictly prohibits Bash wrapper scripts and CPU polling loops.
"""

import os
import argparse
import atexit
import glob
import logging
import pwd
import queue
import re
import shutil
import signal
import socket
import subprocess
import sys
import threading

import infrastructure

_logger = logging.getLogger(__name__)


def load_ignore_file(filepath):
    patterns = []
    if os.path.exists(filepath):
        with open(filepath, "r", encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if line and not line.startswith("#"):
                    patterns.append(re.compile(line))
    return patterns


def is_ignored(path, patterns):
    for pat in patterns:
        if re.search(pat, path):
            return True
    return False


class FailureExtractor:
    """
    State machine that processes log lines in real-time, buffering and extracting
    Tracebacks and error blocks for writing to a filtered log file.
    """

    def __init__(self, output_path, disable_atexit=False):
        self.display_path = os.environ.get("HAMS_REAL_ERROR_LOG") or os.path.abspath(
            os.path.expanduser(output_path)
        )

        if os.environ.get("HAMS_ISOLATED_NS") == "1":
            self.output_path = "/opt/hams/spool/filtered_test.txt"
        else:
            self.output_path = self.display_path

        try:
            os.remove(self.output_path)
        except OSError:
            pass

        self.log_prefix_pattern = re.compile(
            r"^(?:\s*)?\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2},\d{3}"
        )
        self.safe_log_levels = re.compile(r"\b(INFO|WARNING|DEBUG)\b")
        self.test_start_pattern = re.compile(r"Starting Test|^test_.*?\s\.\.\.")

        self.current_context = "Global / Module Loading"
        self.capturing = False
        self.captured_blocks = []
        self.current_block = []
        self._written = False

        if not disable_atexit:
            atexit.register(self.finish_and_write)

    def set_context(self, context_name):
        if self.capturing and self.current_block:
            self.captured_blocks.append((self.current_context, self.current_block))
            self.capturing = False
            self.current_block = []
        self.current_context = context_name

    def process_line(self, line):
        if self.test_start_pattern.search(line):
            self.set_context(line.strip())

        is_log_line = self.log_prefix_pattern.match(line)

        if is_log_line:
            if (
                self.safe_log_levels.search(line)
                or "pika.adapters" in line
                or "AMQPConnector" in line
                or "Cloudflare URL purge API failed for chunk: API fail" in line
                or "Cloudflare Tag purge API failed for chunk: API fail" in line
                or "[BACKUP_WORKER]" in line
            ):
                if self.capturing:
                    self.captured_blocks.append((self.current_context, self.current_block))
                    self.current_block = []
                    self.capturing = False
            else:
                if not self.capturing:
                    self.capturing = True
                self.current_block.append(line)
        else:
            if (
                "======================================================================" in line
                or "Traceback (most recent call last):" in line
                or line.startswith("FAIL: ")
                or line.startswith("ERROR: ")
                or line.startswith("AssertionError")
                or line.startswith("FATAL:")
            ):
                if not self.capturing:
                    self.capturing = True

            if self.capturing:
                self.current_block.append(line)

    def _extract_failed_modules(self):
        modules = set()
        addon_pattern = re.compile(r"odoo\.addons\.([a-zA-Z0-9_]+)")
        filepath_pattern = re.compile(r"\/([a-zA-Z0-9_]+)\/(?:models|controllers|tests|wizard|tools)\/.*?\.py")
        daemon_pattern = re.compile(r"\/daemons\/([a-zA-Z0-9_]+)\/.*?\.py")

        for context, block in self.captured_blocks:
            for match in addon_pattern.findall(context): modules.add(match)
            for match in filepath_pattern.findall(context): modules.add(match)
            for match in daemon_pattern.findall(context): modules.add(f"daemons/{match}")

            for line in block:
                for match in addon_pattern.findall(line): modules.add(match)
                for match in filepath_pattern.findall(line): modules.add(match)
                for match in daemon_pattern.findall(line): modules.add(f"daemons/{match}")

        ignore_list = {"base", "web", "mail", "website", "bus"}
        return sorted([m for m in modules if m not in ignore_list])

    def finish_and_write(self):
        if getattr(self, "_written", False):
            return
        self._written = True

        if self.capturing and self.current_block:
            self.captured_blocks.append((self.current_context, self.current_block))
            self.capturing = False
            self.current_block = []

        grouped_blocks = {}
        for context, block in self.captured_blocks:
            if context not in grouped_blocks:
                grouped_blocks[context] = []
            grouped_blocks[context].extend(block)

        out_dir = os.path.dirname(self.output_path)
        if out_dir:
            os.makedirs(out_dir, exist_ok=True)

        num_failures = len(grouped_blocks)

        with open(self.output_path, "w", encoding="utf-8") as out:
            out.write("=== EXTRACTED TEST FAILURES & ERRORS ===\n")
            if num_failures == 0:
                out.write("\nNo errors or failures detected in the log.\n")
            else:
                failed_modules = self._extract_failed_modules()
                out.write("\n" + "*" * 80 + "\n")
                out.write("SYSTEM DIRECTIVE FOR AI ASSISTANT:\n")
                out.write("The following log contains extracted test failures, tracebacks, and CRITICAL errors from the Odoo test suite.\n")
                out.write("Your immediate task is to analyze these errors, identify the root causes within the provided codebase, and generate the necessary patches to fix these test flaws.\n")

                if failed_modules:
                    out.write("\nTARGET MODULES FOR ANALYSIS:\n")
                    out.write("Based on the tracebacks, the following modules are responsible for or implicated in the failure:\n")
                    for mod in failed_modules:
                        out.write(f"  - {mod}\n")
                    out.write("\nASSUMPTION: The GitHub repository containing these modules has been imported to your environment.\n")
                    out.write("ACTION: Please look up the code for the implicated modules above to diagnose and fix the issue.\n")

                out.write("*" * 80 + "\n")

                for context, block in grouped_blocks.items():
                    if not block: continue
                    out.write("\n" + "=" * 80 + "\n")
                    out.write(f"CONTEXT: {context}\n")
                    out.write("-" * 80 + "\n")
                    for b_line in block:
                        out.write(b_line)
                    out.write("\n")

        print("\n==========================================================")
        if num_failures == 0:
            print("🎉 TEST RUN COMPLETE: No test failures detected.")
        else:
            print(f"🚨 TEST RUN COMPLETE: {num_failures} test failure(s) detected!")
            print(f"📄 Failure details extracted and saved to: {self.display_path}")
        print("==========================================================\n")


def run_cmd(cmd, extractor=None, cwd=None, env=None):
    """
    Executes a shell command blocking natively on IO (Queue.get with OS condition variables)
    eliminating all busy polling loops.
    """
    initial_errors = len(extractor.captured_blocks) if extractor else 0
    if env is None:
        env = dict(os.environ)

    env.setdefault("RABBITMQ_HOST", "localhost")
    env.setdefault("RMQ_HOST", "localhost")
    env.setdefault("REDIS_HOST", "localhost")
    env.setdefault("RMQ_USER", "guest")
    env.setdefault("RMQ_PASS", "guest")
    env.setdefault("ODOO_TEST_CHROME_ARGS", "--headless --no-sandbox --disable-dev-shm-usage --disable-gpu --disable-software-rasterizer --disable-features=ServiceWorker")

    process = subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        bufsize=1,
        start_new_session=True,
        cwd=cwd,
        env=env,
    )

    force_killed = False
    q = queue.Queue()

    def reader():
        try:
            for line in process.stdout:
                q.put(line)
        except Exception as e: # audit-ignore-catch-all
            _logger.error("Reader exception: %s", e)
        q.put(None)

    t = threading.Thread(target=reader, daemon=True)
    t.start()

    idle_seconds = 0
    try:
        while True:
            try:
                # Short blocking wait allows us to check if the primary process died while a child kept stdout open
                line = q.get(timeout=1.0)
                idle_seconds = 0
                if line is None:
                    break

                line_lower = line.lower()
                if ("deprecated" in line_lower and "directive" in line_lower) or "pypdf2" in line_lower:
                    continue

                sys.stdout.write(line)
                sys.stdout.flush()

                if extractor:
                    extractor.process_line(line)

                # OS SIGNAL COORDINATION: Relay JS watchdog alarms directly to the Python test thread
                if "[WATCHDOG ALARM]" in line or "=== TOUR FAILED" in line or "FATAL:" in line:
                    print(f"\n[!] Runner Relay: Detected Watchdog Alarm! Sending SIGUSR1 to unstick Odoo PID {process.pid}...\n")
                    try:
                        os.kill(process.pid, signal.SIGUSR1)
                    except OSError:
                        pass

                if "Hit CTRL-C again or send a second signal" in line:
                    print("\n[!] WARNING: Odoo background thread refused to terminate gracefully. Executing process group kill.\n")
                    os.killpg(os.getpgid(process.pid), signal.SIGKILL)
                    force_killed = True
                    break
            except queue.Empty:
                idle_seconds += 1
                if process.poll() is not None:
                    # The test process died but something (like a Postgres background worker) is holding the pipe open
                    break
                if idle_seconds >= 1200:
                    print("\n[!] WARNING: Test runner hung for 1200 seconds with no output! Killing to continue...")
                    os.killpg(os.getpgid(process.pid), signal.SIGKILL)
                    force_killed = True
                    if extractor:
                        extractor.process_line("CRITICAL: Test execution hung for 1200 seconds. Process forcefully killed.\n")
                    break
    except KeyboardInterrupt:
        print("\n[!] CTRL-C detected! Forcefully terminating the test process group...")
        os.killpg(os.getpgid(process.pid), signal.SIGKILL)
        process.wait()
        sys.exit(1)

    process.wait()

    if force_killed:
        final_errors = len(extractor.captured_blocks) if extractor else 0
        return 1 if final_errors > initial_errors else 0

    return process.returncode


def get_local_modules(base_dir, ignore_patterns):
    mods = []
    for item in os.listdir(base_dir):
        mod_path = os.path.join(base_dir, item)
        if is_ignored(mod_path, ignore_patterns):
            continue
        if os.path.isdir(mod_path) and os.path.isfile(os.path.join(mod_path, "__manifest__.py")):
            mods.append(item)
    return sorted(mods)


def get_addons_path(base_dir):
    paths = ["/usr/lib/python3/dist-packages/odoo/addons", base_dir]
    community_dir = os.path.abspath(os.path.join(base_dir, "..", "hams_community"))
    primary_dir = os.path.abspath(os.path.join(base_dir, "..", "hams_private_primary"))

    if os.path.isdir(community_dir): paths.append(community_dir)
    elif os.path.isdir("/hams_community"): paths.append("/hams_community")
    if os.path.isdir(primary_dir): paths.append(primary_dir)

    return ",".join(paths)


def check_linters(venv_python, base_dir, ignore_filepath, extractor=None, target_modules=None):
    print("[*] Running Manifest Dependency Graph Linter...")
    res_manifest = subprocess.run([venv_python, os.path.join(base_dir, "tools", "check_manifest_dependencies.py"), base_dir])
    if res_manifest.returncode != 0:
        print("🛑 Halting due to manifest load-order violations.")
        sys.exit(1)

    print("[*] Running AST Burn List Linter...")
    burn_script = os.path.join(base_dir, "tools", "check_burn_list.py")
    cmd_burn = [venv_python, burn_script, os.path.join(base_dir, target_modules[0]), "--ignore-file", ignore_filepath] if target_modules and len(target_modules) == 1 else [venv_python, burn_script, base_dir, "--ignore-file", ignore_filepath]

    res_burn = subprocess.run(cmd_burn, capture_output=True, text=True)
    if res_burn.returncode != 0:
        print(res_burn.stdout)
        print(res_burn.stderr)
        print("🛑 Halting due to burn list violations.")
        sys.exit(1)
    else:
        print(res_burn.stdout)

    print("[*] Scanning for Semantic Anchors...")
    res_anchor = subprocess.run([venv_python, os.path.join(base_dir, "tools", "verify_anchors.py"), base_dir], capture_output=True, text=True)
    if res_anchor.returncode != 0:
        print(res_anchor.stdout)
        print(res_anchor.stderr)
        print("🛑 Halting due to anchor violations.")
        sys.exit(1)
    else:
        print(res_anchor.stdout)


def run_daemon_tests(venv_python, base_dir, extractor, ignore_patterns, target_modules):
    print("[*] Executing Standalone Daemon Tests...")
    final_rc = 0
    for mod in target_modules:
        daemon_dir = os.path.join(base_dir, mod, "daemon")
        if not os.path.exists(daemon_dir):
            daemon_dir = os.path.join(base_dir, mod, "daemons")
        if not os.path.exists(daemon_dir) or is_ignored(daemon_dir, ignore_patterns):
            continue

        if any(f.startswith("test_") and f.endswith(".py") for f in os.listdir(daemon_dir)):
            print(f"\n[*] ----------------------------------------------------\n[*] Testing Daemon: {mod}\n[*] ----------------------------------------------------")
            if extractor:
                extractor.set_context(f"Daemon Tests / {mod}")
            cmd = [venv_python, "-m", "unittest", "discover", "-p", "test_*.py", "-v"]
            rc = run_cmd(cmd, extractor, cwd=daemon_dir)
            if rc != 0:
                final_rc = rc
    return final_rc


def rebuild_db(db_name):
    print(f"[*] Dropping and Rebuilding Database Schema ({db_name})...")
    env = dict(os.environ)
    psql_cmd, dropdb_cmd, createdb_cmd = shutil.which("psql") or "psql", shutil.which("dropdb") or "dropdb", shutil.which("createdb") or "createdb"

    subprocess.run([psql_cmd, "postgres", "-c", f"SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{db_name}';"], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=env)
    subprocess.run([dropdb_cmd, "--if-exists", "--force", db_name], check=False, stderr=subprocess.DEVNULL, env=env)
    subprocess.run([createdb_cmd, db_name], check=True, env=env)


def _get_latest_source_mtime(dirs_to_check):
    newest = 0
    ignore_exts = (".pyc", ".pyo", ".dump", ".log", ".swp", ".pot", ".po")
    for d in dirs_to_check:
        for root, dirs, files in os.walk(d):
            dirs[:] = [di for di in dirs if not di.startswith(".") and di not in ("__pycache__", "tests")]
            for f in files:
                if f.endswith(ignore_exts) or f.startswith(".") or f == "filtered_test.txt": continue
                try:
                    mtime = os.path.getmtime(os.path.join(root, f))
                    if mtime > newest: newest = mtime
                except OSError: pass
    return newest


def check_and_restore_cache(db_name, mod_string):
    cache_dir = "/opt/hams/test"
    cache_file = os.path.join(cache_dir, "db_cache_master.dump")
    base_dir = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
    search_bases = [base_dir]

    for opt in [os.path.abspath(os.path.join(base_dir, "..", "hams_community")), "/hams_community"]:
        if os.path.exists(opt): search_bases.append(opt)

    dirs_to_check = []
    for sb in search_bases:
        try:
            for item in os.listdir(sb):
                item_path = os.path.join(sb, item)
                if os.path.isdir(item_path) and os.path.isfile(os.path.join(item_path, "__manifest__.py")):
                    dirs_to_check.append(item_path)
        except OSError: pass

    newest_mtime = _get_latest_source_mtime(dirs_to_check)
    for cf in [os.path.join(base_dir, "tools", "infrastructure.py"), os.path.join(base_dir, "tools", "test.py"), os.path.join(base_dir, "deploy", "bootstrap_daemon_keys.py")]:
        if os.path.exists(cf):
            try: newest_mtime = max(newest_mtime, os.path.getmtime(cf))
            except OSError: pass

    cache_valid = False
    if os.path.exists(cache_file):
        try:
            sz = os.path.getsize(cache_file)
            print(f"[*] Found existing DB cache: {sz} bytes.")
            if sz > 5000000 and os.path.getmtime(cache_file) >= newest_mtime:
                cache_valid = True
        except OSError: pass

    if cache_valid:
        mod_file = cache_file.replace(".dump", ".modules")
        if os.path.exists(mod_file):
            with open(mod_file, "r") as f:
                if f.read().strip() != mod_string: cache_valid = False
        else:
            cache_valid = False

    if cache_valid:
        print(f"[*] Valid DB cache found ({cache_file}). Restoring (Parallel)...")
        rebuild_db(db_name)
        res = subprocess.run([shutil.which("pg_restore") or "pg_restore", "-d", db_name, "-O", "-x", "-j", "4", cache_file], capture_output=True, text=True)
        if res.returncode == 0:
            print("[*] DB restored from cache.")
            filestore_tar = cache_file.replace(".dump", ".filestore.tar.gz")
            if os.path.exists(filestore_tar):
                print("[*] Restoring Filestore...")
                filestore_base = "/var/lib/odoo/.local/share/Odoo/filestore"
                os.makedirs(filestore_base, exist_ok=True)
                subprocess.run(["tar", "-xzf", filestore_tar, "-C", filestore_base])
            return True, cache_file
        else:
            print(f"[*] WARNING: pg_restore failed:\n{res.stderr}")

    if os.path.exists(cache_file):
        try: os.remove(cache_file)
        except OSError: pass

    print("[*] DB cache missing or invalid. Rebuilding...")
    rebuild_db(db_name)
    return False, cache_file


def save_db_cache(db_name, cache_file, mod_string):
    print(f"[*] Caching newly initialized DB to {cache_file}...")
    try: os.makedirs(os.path.dirname(cache_file), exist_ok=True)
    except Exception as e: # audit-ignore-catch-all
        _logger.warning("Failed to create cache dir: %s", e)

    env = dict(os.environ)
    try:
        with open(cache_file, "wb") as f:
            res = subprocess.run([shutil.which("pg_dump") or "pg_dump", "-Fc", "-Z", "1", db_name], stdout=f, stderr=subprocess.PIPE, env=env)
        if res.returncode == 0 and os.path.getsize(cache_file) > 5000000:
            print("[*] DB cached successfully.")
            with open(cache_file.replace(".dump", ".modules"), "w") as mf: mf.write(mod_string)
            filestore_path = f"/var/lib/odoo/.local/share/Odoo/filestore/{db_name}"
            if os.path.exists(filestore_path):
                print("[*] Caching Filestore...")
                subprocess.run(["tar", "-czf", cache_file.replace(".dump", ".filestore.tar.gz"), "-C", os.path.dirname(filestore_path), db_name])
        else:
            try: os.remove(cache_file)
            except OSError: pass
    except Exception as e: # audit-ignore-catch-all
        _logger.error("Failed to execute pg_dump: %s", e)


def setup_namespace_and_run_tests(real_error_log, sys_args):
    """
    Pure-Python OS-level namespace bootstrapping.
    Executes entirely without Bash wrappers, leveraging `os.setuid` for micro-privileges.
    """
    # 1. Loopback Network
    subprocess.run(["ip", "link", "set", "lo", "up"], check=True)

    # 2. Ephemeral OverlayFS File System
    subprocess.run(["mount", "--make-rprivate", "/"], check=True)
    subprocess.run(["mount", "-t", "tmpfs", "tmpfs", "/mnt"], check=True)
    for d in ["/mnt/upper", "/mnt/work", "/mnt/host_test_dir", "/opt/hams/test"]:
        os.makedirs(d, exist_ok=True)
    subprocess.run(["mount", "--bind", "/opt/hams/test", "/mnt/host_test_dir"], check=True)

    base_dir = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))

    def _safe_run(cmd, **kw): return subprocess.run(cmd, check=True, **kw)
    infrastructure.apply_production_directories(_safe_run, environment="test", dest_dir="/mnt/upper")
    infrastructure.write_env_files("/opt/hams/etc", dict(os.environ), _safe_run, dest_dir="/mnt/upper")
    infrastructure.provision_static_files(_safe_run, dict(os.environ), environment="test", dest_dir="/mnt/upper")
    infrastructure.execute_hooks("test", _safe_run, dict(os.environ), dest_dir="/mnt/upper")

    for item in os.listdir("/mnt/upper"):
        if item == "mnt": continue
        os.makedirs(f"/mnt/work/{item}", exist_ok=True)
        subprocess.run(["mount", "-t", "overlay", "overlay", "-o", f"lowerdir=/{item},upperdir=/mnt/upper/{item},workdir=/mnt/work/{item}", f"/{item}"], check=True)

    subprocess.run(["mount", "--bind", base_dir, base_dir], check=True)
    subprocess.run(["mount", "-o", "remount,bind,ro", base_dir], check=True)

    for extra_dir in [os.path.join(base_dir, "..", "hams_community"), "/hams_community"]:
        if os.path.isdir(extra_dir):
            real_dir = os.path.realpath(extra_dir)
            subprocess.run(["mount", "--bind", real_dir, real_dir], check=True)
            subprocess.run(["mount", "-o", "remount,bind,ro", real_dir], check=True)

    # 3. PostgreSQL Sandboxing
    pg_bins = glob.glob("/usr/lib/postgresql/*/bin/initdb")
    if not pg_bins:
        print("❌ ERROR: Could not locate PostgreSQL initdb.")
        sys.exit(1)
    pg_bin_dir = os.path.dirname(sorted(pg_bins)[-1]) + "/"
    pg_data, pg_sock = "/opt/hams/pgdata", "/opt/hams/pgsock"

    pg_user = pwd.getpwnam("postgres")
    def preexec_pg():
        os.setgid(pg_user.pw_gid)
        os.setuid(pg_user.pw_uid)

    subprocess.run([f"{pg_bin_dir}initdb", "-D", pg_data], preexec_fn=preexec_pg, check=True, stdout=subprocess.DEVNULL)
    # The -w flag makes pg_ctl natively BLOCK until ready. No polling loops needed!
    subprocess.run([f"{pg_bin_dir}pg_ctl", "-D", pg_data, "-o", f"-c listen_addresses= -c unix_socket_directories={pg_sock} -c fsync=off -c synchronous_commit=off -c full_page_writes=off", "-w", "start"], preexec_fn=preexec_pg, check=True, stdout=subprocess.DEVNULL)

    orig_user = os.environ.get("SUDO_USER", "odoo")
    p = subprocess.Popen([f"{pg_bin_dir}psql", "-h", pg_sock, "-d", "postgres"], stdin=subprocess.PIPE, preexec_fn=preexec_pg, text=True, stdout=subprocess.DEVNULL)
    p.communicate(f"CREATE ROLE odoo WITH SUPERUSER LOGIN PASSWORD 'odoo'; CREATE ROLE {orig_user} WITH SUPERUSER LOGIN;")
    p.wait()

    # 4. Redis Sandboxing
    redis_user = pwd.getpwnam("redis")
    redis_proc = subprocess.Popen(["redis-server", "--daemonize", "no"], preexec_fn=lambda: (os.setgid(redis_user.pw_gid), os.setuid(redis_user.pw_uid)), stdout=subprocess.DEVNULL)

    # 5. RabbitMQ Sandboxing
    subprocess.run(["systemctl", "stop", "rabbitmq-server"], check=False, stderr=subprocess.DEVNULL)
    subprocess.run(["pkill", "-u", "rabbitmq"], check=False, stderr=subprocess.DEVNULL)
    subprocess.run(["pkill", "epmd"], check=False, stderr=subprocess.DEVNULL)

    os.makedirs("/var/lib/rabbitmq", exist_ok=True)
    with open("/var/lib/rabbitmq/.erlang.cookie", "w") as f:
        f.write("HAMS_TEST_RABBITMQ_COOKIE_12345")

    rmq_user = pwd.getpwnam("rabbitmq")
    os.chown("/var/lib/rabbitmq/.erlang.cookie", rmq_user.pw_uid, rmq_user.pw_gid)
    os.chmod("/var/lib/rabbitmq/.erlang.cookie", 0o400)

    def preexec_rmq():
        os.setgid(rmq_user.pw_gid)
        os.setuid(rmq_user.pw_uid)
        os.environ["HOME"] = "/var/lib/rabbitmq"

    subprocess.run(["rabbitmq-server", "-detached"], preexec_fn=preexec_rmq, check=True, stdout=subprocess.DEVNULL)
    # Native blocking await. No polling loops needed!
    subprocess.run(["rabbitmqctl", "await_online_nodes", "1"], preexec_fn=preexec_rmq, check=True, timeout=120, stdout=subprocess.DEVNULL)
    subprocess.run(["rabbitmqctl", "start_app"], preexec_fn=preexec_rmq, check=True, stdout=subprocess.DEVNULL)

    # 6. Execute Inner Odoo Test Suite
    os.environ["PYTHONDONTWRITEBYTECODE"] = "1"
    os.environ["HAMS_ISOLATED_NS"] = "1"
    os.environ["PGHOST"] = pg_sock
    os.environ["ODOO_TEST_CHROME_ARGS"] = "--headless --no-sandbox --disable-dev-shm-usage --disable-gpu --disable-software-rasterizer --disable-features=ServiceWorker"
    os.environ["HAMS_REAL_ERROR_LOG"] = real_error_log

    odoo_user = pwd.getpwnam("odoo")
    def preexec_odoo():
        os.setgid(odoo_user.pw_gid)
        os.setuid(odoo_user.pw_uid)

    test_cmd = [sys.executable, os.path.abspath(__file__)] + sys_args
    ret = subprocess.run(test_cmd, preexec_fn=preexec_odoo).returncode

    # 7. Graceful Ephemeral Teardown
    subprocess.run(["rabbitmqctl", "stop"], preexec_fn=preexec_rmq, check=False, stdout=subprocess.DEVNULL)
    redis_proc.terminate()
    subprocess.run([f"{pg_bin_dir}pg_ctl", "-D", pg_data, "-m", "fast", "stop"], preexec_fn=preexec_pg, check=False, stdout=subprocess.DEVNULL)

    if os.path.exists("/opt/hams/spool/filtered_test.txt"):
        os.makedirs(os.path.dirname(real_error_log), exist_ok=True)
        shutil.copy2("/opt/hams/spool/filtered_test.txt", real_error_log)
        orig_uid = pwd.getpwnam(orig_user).pw_uid
        os.chown(real_error_log, orig_uid, -1)

    for prof in glob.glob("/opt/hams/spool/*.prof"):
        dst = os.path.join(os.path.dirname(real_error_log), os.path.basename(prof))
        shutil.copy2(prof, dst)
        os.chown(dst, orig_uid, -1)

    for cf in ["db_cache_master.dump", "db_cache_master.modules", "db_cache_master.filestore.tar.gz"]:
        src = f"/mnt/upper/opt/hams/test/{cf}"
        dst = f"/mnt/host_test_dir/{cf}"
        if os.path.exists(src):
            shutil.copy2(src, dst)
            os.chmod(dst, 0o666)

    sys.exit(ret)


def provision_jules(base_dir):
    """Provisions a pre-isolated Jules VM environment"""
    orig_user = os.environ.get("SUDO_USER") or os.environ.get("USER")
    def run_sudo(cmd): subprocess.run(["sudo", "bash", "-c", cmd], check=True)

    print("[*] Clearing port 8069 bindings...")
    run_sudo("kill $(lsof -t -i :8069) 2>/dev/null || true")

    print("[*] Configuring local PostgreSQL...")
    pg_bins = glob.glob("/usr/lib/postgresql/*/bin/initdb")
    if not pg_bins:
        print("❌ ERROR: PostgreSQL initdb not found.")
        sys.exit(1)
    pg_bin_dir = os.path.dirname(sorted(pg_bins)[-1]) + "/"
    pg_data, pg_socket = "/opt/hams/pgdata", "/opt/hams/pgsock"

    run_sudo("systemctl stop postgresql || true")
    run_sudo(f"mkdir -p {pg_data} {pg_socket}")
    run_sudo(f"chown -R {orig_user}:{orig_user} {pg_data} {pg_socket}")
    run_sudo(f"chmod 700 {pg_data}; chmod 2775 {pg_socket}")

    res = subprocess.run(["sudo", "ls", "-A", pg_data], capture_output=True, text=True)
    if not res.stdout.strip():
        run_sudo(f"su -s /bin/bash {orig_user} -c '{pg_bin_dir}initdb -D {pg_data}'")

    run_sudo(f"su -s /bin/bash {orig_user} -c \"{pg_bin_dir}pg_ctl -D {pg_data} -m fast stop\" || true")
    run_sudo(f"rm -f {pg_data}/postmaster.pid || true")
    run_sudo(f"su -s /bin/bash {orig_user} -c \"{pg_bin_dir}pg_ctl -D {pg_data} -o '-c listen_addresses= -c unix_socket_directories={pg_socket} -c fsync=off -c synchronous_commit=off -c full_page_writes=off' -w start\"")
    run_sudo(f"echo \"CREATE ROLE odoo WITH SUPERUSER LOGIN PASSWORD 'odoo'; CREATE ROLE {orig_user} WITH SUPERUSER LOGIN;\" | su -s /bin/bash {orig_user} -c 'PGUSER={orig_user} {pg_bin_dir}psql -h {pg_socket} -d postgres' || true")

    print("[*] Starting local Redis and RabbitMQ...")
    run_sudo("systemctl start redis-server || true")
    run_sudo("systemctl start rabbitmq-server || true")

    os.environ["PGHOST"] = pg_socket
    os.environ["HAMS_ISOLATED_NS"] = "1"

    def teardown():
        subprocess.run(["sudo", "su", "-s", "/bin/bash", orig_user, "-c", f"{pg_bin_dir}pg_ctl -D {pg_data} -m fast stop"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    atexit.register(teardown)


def main():
    if os.environ.get("HAMS_ISOLATED_NS") != "1" and not os.environ.get("IN_JULES_VM") and not os.environ.get("JULES_SESSION_ID"):
        if "--internal-ns-init" in sys.argv:
            # Phase 2: Execute completely within Python (No bash script interpolation)
            real_error_log = os.environ.get("HAMS_REAL_ERROR_LOG")
            sys_args = [arg for arg in sys.argv[1:] if arg != "--internal-ns-init"]
            setup_namespace_and_run_tests(real_error_log, sys_args)
            return

        parser = argparse.ArgumentParser()
        parser.add_argument("-e", "--error-log", default="~/tmp/filtered_test.txt")
        args, _ = parser.parse_known_args()

        real_error_log = os.path.abspath(os.path.expanduser(args.error_log))
        print("[*] Routing test execution to isolated Python namespace...")

        os.environ["HAMS_REAL_ERROR_LOG"] = real_error_log
        exec_cmd = ["unshare", "-m", sys.executable, os.path.abspath(__file__), "--internal-ns-init"] + sys.argv[1:]

        # os.execvpe completely replaces the current process, passing control natively
        os.execvpe("unshare", exec_cmd, os.environ)
        return

    os.environ["PYTHONWARNINGS"] = "ignore::DeprecationWarning"
    os.environ.setdefault("ODOO_TEST_CHROME_ARGS", "--headless --no-sandbox --disable-dev-shm-usage --disable-gpu --disable-software-rasterizer --disable-features=ServiceWorker")

    base_dir = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
    env_path = os.path.join(base_dir, "deploy", "env")
    if os.path.exists(env_path):
        with open(env_path, "r", encoding="utf-8") as f:
            for line in f:
                if "=" in line and not line.startswith("#"):
                    k, v = line.split("=", 1)
                    os.environ.setdefault(k.strip(), v.strip("'\" \n"))

    os.environ.setdefault("PGHOST", "localhost")
    os.environ.setdefault("PGUSER", os.environ.get("DB_USER") or os.environ.get("POSTGRES_USER") or "odoo")
    os.environ.setdefault("PGPASSWORD", os.environ.get("DB_PASSWORD") or os.environ.get("POSTGRES_PASSWORD") or "odoo")
    os.environ.setdefault("RABBITMQ_HOST", "localhost")
    os.environ.setdefault("REDIS_HOST", "localhost")

    parser = argparse.ArgumentParser(formatter_class=argparse.RawTextHelpFormatter)
    parser.add_argument("-m", "--mode", choices=["standard", "integration", "individual", "xml", "downloads"], default="standard")
    parser.add_argument("-d", "--db", default="hams_test")
    parser.add_argument("-u", "--module")
    parser.add_argument("-e", "--error-log", default="~/tmp/filtered_test.txt")
    parser.add_argument("-c", "--config", default="ignore_list.txt")
    parser.add_argument("--daemon")
    parser.add_argument("--provision-jules", action="store_true")
    parser.add_argument("--already-provisioned", action="store_true")
    parser.add_argument("--profile", action="store_true")
    args = parser.parse_args()

    is_jules = bool(os.environ.get("IN_JULES_VM")) or bool(os.environ.get("JULES_SESSION_ID"))
    if is_jules and (args.provision_jules or args.already_provisioned):
        provision_jules(base_dir)

    venv_python = "/usr/bin/python3" if is_jules else os.path.join(base_dir, ".venv", "bin", "python")
    odoo_bin = "/usr/bin/odoo"
    addons_path = get_addons_path(base_dir)

    ignore_filepath = os.path.join(base_dir, args.config)
    ignore_patterns = load_ignore_file(ignore_filepath)

    target_modules = [m.strip() for m in args.module.split(",")] if args.module else get_local_modules(base_dir, ignore_patterns)
    if not target_modules:
        print("❌ ERROR: No modules found.")
        sys.exit(1)

    mod_string = "base," + ",".join(target_modules)
    test_tags = ",".join([f"/{m}" for m in target_modules])

    def get_odoo_test_cmd(suffix=""):
        cmd = [venv_python]
        if args.profile:
            cmd.extend(["-m", "cProfile", "-o", f"/opt/hams/spool/odoo_test{suffix}.prof"])
        return cmd

    extractor = FailureExtractor(args.error_log)
    print(f"==========================================================\n 🧪 ODOO TEST RUNNER [{args.mode.upper()} MODE]\n==========================================================")
    final_rc = 0

    if args.mode == "standard":
        check_linters(venv_python, base_dir, ignore_filepath, extractor, target_modules)
        final_rc = run_daemon_tests(venv_python, base_dir, extractor, ignore_patterns, target_modules)

        restored, cache_file = check_and_restore_cache(args.db, mod_string)
        if not restored:
            rc_init = run_cmd([venv_python, odoo_bin, "--addons-path", addons_path, "-d", args.db, "-i", mod_string, "--stop-after-init", "--workers=0", "--max-cron-threads=0"], extractor)
            if rc_init != 0:
                print("❌ ERROR: DB init failed!")
                if extractor: extractor.finish_and_write()
                sys.exit(rc_init)
            save_db_cache(args.db, cache_file, mod_string)

        cmd = get_odoo_test_cmd() + [odoo_bin, "--addons-path", addons_path, "--dev=all", "-d", args.db, "-u", mod_string, "--test-enable", "--test-tags", test_tags + ",-integration", "--stop-after-init", "--workers=0", "--max-cron-threads=0"]
        rc_odoo = run_cmd(cmd, extractor)
        if rc_odoo != 0: final_rc = rc_odoo

    elif args.mode == "integration":
        check_linters(venv_python, base_dir, ignore_filepath, extractor, target_modules)
        final_rc = run_daemon_tests(venv_python, base_dir, extractor, ignore_patterns, target_modules)
        os.environ["HAMS_INTEGRATION_MODE"] = "1"

        restored, cache_file = check_and_restore_cache(args.db, mod_string)
        if not restored:
            rc_init = run_cmd([venv_python, odoo_bin, "--addons-path", addons_path, "-d", args.db, "-i", mod_string, "--test-enable", "--test-tags", "/__skip_init__", "--stop-after-init", "--workers=0", "--max-cron-threads=0"], extractor)
            if rc_init == 0: save_db_cache(args.db, cache_file, mod_string)
            else: final_rc = rc_init

        if final_rc == 0:
            with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
                s.bind(("", 0))
                free_port = s.getsockname()[1]

            print(f"[*] Starting background Odoo on port {free_port}...")
            odoo_proc = subprocess.Popen([venv_python, odoo_bin, "--addons-path", addons_path, "-d", args.db, "--db-filter", f"^{args.db}$", "--workers=0", "--max-cron-threads=0", "--http-port", str(free_port), "--log-level=warn"], stdout=subprocess.PIPE, text=True)

            ready_event = threading.Event()
            def stream_odoo():
                try:
                    for line in odoo_proc.stdout:
                        sys.stdout.write(line)
                        sys.stdout.flush()
                        if "http service (werkzeug) running on" in line.lower() or "modules loaded." in line.lower() or "running on" in line.lower():
                            ready_event.set()
                except Exception as e: # audit-ignore-catch-all
                    logging.error("Error in stream_odoo: %s", e)

            threading.Thread(target=stream_odoo, daemon=True).start()

            # Smart wait: check process vitality while waiting for the event
            is_ready = False
            for _ in range(480):  # 120 seconds max (480 * 0.25s)
                if ready_event.wait(timeout=0.25):
                    is_ready = True
                    break
                if odoo_proc.poll() is not None:
                    print(f"❌ ERROR: Odoo background process crashed prematurely with code {odoo_proc.returncode}!")
                    break

            if not is_ready:
                print(f"❌ ERROR: Odoo failed to start on port {free_port} within 120 seconds or crashed.")
                os.killpg(os.getpgid(odoo_proc.pid), signal.SIGKILL)
                sys.exit(1)

            daemon_env = os.environ.copy()
            daemon_env.update({"ODOO_URL": f"http://localhost:{free_port}", "DB_NAME": args.db, "ODOO_USER": "admin", "ODOO_PASSWORD": "admin"})

            d_procs = []
            for mod in target_modules:
                for d in ["daemon", "daemons"]:
                    dd = os.path.join(base_dir, mod, d)
                    if os.path.exists(dd):
                        for f in os.listdir(dd):
                            if f.endswith(".py") and not f.startswith("test_") and not f.startswith("__"):
                                d_procs.append(subprocess.Popen([venv_python, os.path.join(dd, f)], env=daemon_env, stdout=subprocess.DEVNULL))

            test_cmd = get_odoo_test_cmd("_integration") + [odoo_bin, "--addons-path", addons_path, "-d", args.db, "-u", mod_string, "--test-enable", "--test-tags", test_tags + ",-standard", "--stop-after-init", "--workers=0", "--max-cron-threads=0"]
            rc_odoo = run_cmd(test_cmd, extractor, env=daemon_env)
            if rc_odoo != 0: final_rc = rc_odoo

            for p in d_procs: p.terminate()
            os.killpg(os.getpgid(odoo_proc.pid), signal.SIGKILL)

    elif args.mode == "individual":
        check_linters(venv_python, base_dir, ignore_filepath, extractor, target_modules)
        for mod in target_modules:
            restored, cache_file = check_and_restore_cache(args.db, mod)
            if not restored:
                rc_init = run_cmd([venv_python, odoo_bin, "--addons-path", addons_path, "--dev=all", "-d", args.db, "-i", mod, "--stop-after-init", "--workers=0", "--max-cron-threads=0"], extractor)
                if rc_init == 0: save_db_cache(args.db, cache_file, mod)
                else:
                    final_rc = 1
                    continue

            rc = run_cmd(get_odoo_test_cmd(f"_{mod}") + [odoo_bin, "--addons-path", addons_path, "--dev=all", "-d", args.db, "-u", mod, "--test-enable", "--test-tags", f"/{mod}", "--stop-after-init", "--workers=0", "--max-cron-threads=0"], extractor)
            if rc != 0: final_rc = 1

    sys.exit(final_rc)

if __name__ == "__main__":
    main()
