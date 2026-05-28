#!/usr/bin/env python3
"""
Unified Pure-Python Odoo Test Runner for Hams.com
Combines test execution, integration modes, and real-time failure extraction.
Strictly prohibits Bash wrapper scripts and CPU polling loops.
"""

import infrastructure
import argparse
import atexit
import contextlib
import glob
import logging
import os
import pwd
import queue
import re
import shutil
import signal
import socket
import subprocess
import sys
import threading
import time

@contextlib.contextmanager
def micro_privilege(username):
    """
    Temporarily drops Effective privileges to the specified user using setresuid/setresgid.
    Restores Root privileges securely upon exiting the context block.
    """
    if os.geteuid() != 0:
        yield
        return

    user_info = pwd.getpwnam(username)
    target_uid = user_info.pw_uid
    target_gid = user_info.pw_gid

    orig_ruid, orig_euid, orig_suid = os.getresuid()
    orig_rgid, orig_egid, orig_sgid = os.getresgid()

    try:
        os.setresgid(orig_rgid, target_gid, orig_sgid)
        os.setresuid(orig_ruid, target_uid, orig_suid)
        yield
    finally:
        os.setresuid(orig_ruid, orig_euid, orig_suid)
        os.setresgid(orig_rgid, orig_egid, orig_sgid)

# Local modules resolve natively without sys.path hacks.

_logger = logging.getLogger(__name__)

class VirtualClockThread(threading.Thread):
    """
    A CPU-time equivalent clock that suppresses massive jumps in wall-clock time
    caused by the VM being suspended or heavily timeshared.
    """
    def __init__(self):
        super().__init__(daemon=True)
        self.vtime = 0.0
        self.last_real = time.time()
        self._lock = threading.Lock()

    def run(self):
        while True:
            time.sleep(0.1)
            now = time.time()
            delta = now - self.last_real
            self.last_real = now
            with self._lock:
                self.vtime += min(delta, 0.5)

    def time(self):
        with self._lock:
            return self.vtime

global_vclock = VirtualClockThread()
global_vclock.start()

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

    def __init__(self, log_dir, disable_atexit=False):
        base_dir = os.environ.get("HAMS_REAL_LOG_DIRECTORY") or os.path.abspath(
            os.path.expanduser(log_dir)
        )
        os.makedirs(base_dir, exist_ok=True)
        try:
            os.chmod(base_dir, 0o1777)
        except OSError:
            pass
        self.display_path = os.path.join(base_dir, "filtered_test.txt")

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


def robust_reap(pid):
    """
    Process reaper that targets the process group with SIGTERM,
    waits using a 30-second poll-and-half-second-sleep, and escalates to SIGKILL.
    """
    print(f"\n[*] [REAPER] Initiating robust reaper for PID {pid}...")
    try:
        subprocess.run(["pkill", "-TERM", "-f", "chrome"], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

        pgid = os.getpgid(pid)
        print(f"[*] [REAPER] Sending SIGTERM to Process Group {pgid}")
        os.killpg(pgid, signal.SIGTERM)

        start_time = time.time()
        while time.time() - start_time < 30.0:
            try:
                os.kill(pid, 0)
            except OSError:
                print(f"[*] [REAPER] Process {pid} confirmed dead.")
                return
            time.sleep(0.5)

        print(f"[*] [REAPER] Process {pid} did not exit after SIGTERM. Sending SIGKILL to Process Group {pgid}")
        os.killpg(pgid, signal.SIGKILL)
        subprocess.run(["pkill", "-KILL", "-f", "chrome"], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    except OSError as e:
        print(f"[*] [REAPER] Error during reap: {e}")


def run_cmd(cmd, extractor=None, cwd=None, env=None):
    initial_errors = len(extractor.captured_blocks) if extractor else 0
    if env is None:
        env = dict(os.environ)

    env.setdefault("RABBITMQ_HOST", "localhost")
    env.setdefault("RMQ_HOST", "localhost")
    env.setdefault("REDIS_HOST", "localhost")
    env.setdefault("RMQ_USER", "guest")
    env.setdefault("RMQ_PASS", "guest")
    host_tmp_dir = os.environ.get("HAMS_REAL_LOG_DIRECTORY", "/var/tmp")
    os.makedirs(host_tmp_dir, exist_ok=True)
    try:
        os.chmod(host_tmp_dir, 0o1777)
    except OSError:
        pass
    env.setdefault("ODOO_TEST_CHROME_ARGS", f"--headless --no-sandbox --disable-dev-shm-usage --disable-gpu --disable-software-rasterizer --disable-features=ServiceWorker,SharedWorker,DialMediaRouteProvider --user-data-dir={host_tmp_dir}")
    env.setdefault("DBUS_SESSION_BUS_ADDRESS", "/dev/null")

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

    print(f"\n[*] [DEBUG-RUNNER] Subprocess spawned: PID {process.pid}, PGID {os.getpgid(process.pid)}")
    print(f"[*] [DEBUG-RUNNER] Executing command: {' '.join(cmd)}")

    force_killed = False
    q = queue.Queue()
    last_output_time = time.time()

    def reader():
        print(f"[*] [DEBUG-RUNNER] IO Reader thread started for PID {process.pid}")
        try:
            for line in process.stdout:
                q.put(line)
        except Exception as e: # audit-ignore-catch-all
            _logger.error("Reader exception: %s", e)
            print(f"[*] [DEBUG-RUNNER] IO Reader thread exception: {e}")
        q.put(None)
        print(f"[*] [DEBUG-RUNNER] IO Reader thread concluded for PID {process.pid}")

    t = threading.Thread(target=reader, daemon=True)
    t.start()

    try:
        while True:
            try:
                # Short blocking wait allows us to check if the primary process died while a child kept stdout open
                line = q.get(timeout=1.0)
                if line is None:
                    print("[*] [DEBUG-RUNNER] Received EOF sentinel from IO Reader thread.")
                    break

                last_output_time = time.time()

                if "@t-esc" in line and "deprecated" in line.lower():
                    continue

                sys.stdout.write(line)
                sys.stdout.flush()

                if extractor:
                    extractor.process_line(line)

                line_lower = line.lower()

                if "[watchdog alarm]" in line_lower:
                    print("\n[!] FATAL JS WATCHDOG ALARM DETECTED in JS! Allowing Odoo framework to process the dump and continue...\n")
            except queue.Empty:
                if process.poll() is not None:
                    sys.stdout.write(f"[*] [DEBUG-RUNNER] Process {process.pid} exited with {process.poll()}, but stdout pipe remains open. Breaking loop.\n")
                    sys.stdout.flush()
                    # The test process died but something (like a Postgres background worker) is holding the pipe open
                    break

                if time.time() - last_output_time > 60.0:
                    print("\n[!] TEST TIMEOUT: No output received for 60 seconds. Tour or test likely hung. Terminating...\n")
                    robust_reap(process.pid)
                    force_killed = True
                    break
    except KeyboardInterrupt:
        print("\n[!] CTRL-C detected! Forcefully terminating the test process...")
        robust_reap(process.pid)
        process.wait()
        sys.exit(1)

    print(f"[*] [DEBUG-RUNNER] Waiting for process {process.pid} to cleanly terminate...")
    process.wait()
    print(f"[*] [DEBUG-RUNNER] Process {process.pid} terminated with return code {process.returncode}.")

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
    primary_dir = os.path.abspath(os.path.join(base_dir, "..", "hams_com"))

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

    print("[*] Running JavaScript Syntax Linter...")
    js_linter = os.path.join(base_dir, "tools", "check_js_syntax.py")
    target_dirs = [os.path.join(base_dir, m) for m in target_modules]
    cmd_js = [venv_python, js_linter, "--ignore-file", ignore_filepath] + target_dirs
    res_js = subprocess.run(cmd_js, capture_output=True, text=True)
    if res_js.returncode != 0:
        print(res_js.stdout)
        print(res_js.stderr)
        print("🛑 Halting due to JavaScript syntax errors.")
        sys.exit(1)
    else:
        print(res_js.stdout)


def wait_for_port(port, name, host="127.0.0.1", timeout=60.0):
    print(f"[*] Waiting for {name} on {host}:{port} to open...")
    start_time = global_vclock.time()
    while global_vclock.time() - start_time < timeout:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            sock.settimeout(1.0)
            if sock.connect_ex((host, port)) == 0:
                print(f"[*] {name} is ready.")
                return True
        time.sleep(0.5)
    print(f"❌ ERROR: {name} did not open port {port} within {timeout} seconds.")
    return False

def wait_for_socket(sock_path, name, timeout=60.0):
    print(f"[*] Waiting for {name} unix socket {sock_path} to open...")
    start_time = global_vclock.time()
    while global_vclock.time() - start_time < timeout:
        if os.path.exists(sock_path):
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
                sock.settimeout(1.0)
                try:
                    sock.connect(sock_path)
                    print(f"[*] {name} socket is ready.")
                    return True
                except OSError:
                    pass
        time.sleep(0.5)
    print(f"❌ ERROR: {name} socket {sock_path} did not open within {timeout} seconds.")
    return False


def get_pg_bin(name):
    """Locate a PostgreSQL binary reliably across different distributions."""
    paths = glob.glob(f"/usr/lib/postgresql/*/bin/{name}")
    if paths:
        return sorted(paths)[-1]
    res = shutil.which(name)
    if not res:
        for p in [f"/usr/bin/{name}", f"/usr/local/bin/{name}"]:
            if os.path.exists(p): return p
        raise FileNotFoundError(f"Could not find PostgreSQL binary: {name}")
    return res


def rebuild_db(db_name):
    print(f"[*] Dropping and Rebuilding Database Schema ({db_name})...")
    env = dict(os.environ)

    print("[*] Flushing persistent daemons (Redis / RabbitMQ)...")
    subprocess.run(["redis-cli", "flushall"], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=env)

    is_jules = bool(os.environ.get("IN_JULES_VM")) or bool(os.environ.get("JULES_SESSION_ID"))
    if is_jules:
        try:
            subprocess.run(["sudo", "rabbitmqctl", "stop_app"], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            subprocess.run(["sudo", "rabbitmqctl", "reset"], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            subprocess.run(["sudo", "rabbitmqctl", "start_app"], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            subprocess.run(["sudo", "systemctl", "stop", "dx.firehose.service", "adif.processor.service", "qrz.scraper.service"], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        except Exception as e: # audit-ignore-catch-all
            _logger.warning("Daemon flush exception: %s", e)

    try:
        psql_cmd = get_pg_bin("psql")
        dropdb_cmd = get_pg_bin("dropdb")
        createdb_cmd = get_pg_bin("createdb")
    except FileNotFoundError as e:
        print(f"❌ ERROR: {e}")
        sys.exit(1)

    subprocess.run([psql_cmd, "postgres", "-c", f"SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{db_name}';"], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=env)
    subprocess.run([dropdb_cmd, "--if-exists", "--force", db_name], check=False, stderr=subprocess.DEVNULL, env=env)
    subprocess.run([createdb_cmd, db_name], check=True, env=env)


def setup_namespace_and_run_tests(real_log_dir, sys_args):
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

    # Bind mount host tmp directory AFTER overlayfs so it isnt shadowed
    host_tmp_dir = real_log_dir if real_log_dir else "/var/tmp"
    os.makedirs(host_tmp_dir, exist_ok=True)
    try:
        os.chmod(host_tmp_dir, 0o1777)
    except OSError:
        pass
    os.makedirs("/var/tmp", exist_ok=True)
    subprocess.run(["mount", "--bind", host_tmp_dir, "/var/tmp"], check=True)

    subprocess.run(["mount", "--bind", base_dir, base_dir], check=True)
    subprocess.run(["mount", "-o", "remount,bind,ro", base_dir], check=True)

    for extra_dir in [os.path.join(base_dir, "..", "hams_community"), "/hams_community"]:
        if os.path.isdir(extra_dir):
            real_dir = os.path.realpath(extra_dir)
            subprocess.run(["mount", "--bind", real_dir, real_dir], check=True)
            subprocess.run(["mount", "-o", "remount,bind,ro", real_dir], check=True)

    # 3. PostgreSQL Sandboxing
    try:
        initdb_cmd = get_pg_bin("initdb")
        pg_ctl_cmd = get_pg_bin("pg_ctl")
        psql_cmd = get_pg_bin("psql")
    except FileNotFoundError as e:
        print(f"❌ ERROR: {e}")
        sys.exit(1)

    pg_data, pg_sock = "/opt/hams/pgdata", "/opt/hams/pgsock"

    orig_user = os.environ.get("SUDO_USER", "odoo")
    pg_user = pwd.getpwnam("postgres")
    def preexec_pg():
        os.setresgid(pg_user.pw_gid, pg_user.pw_gid, pg_user.pw_gid)
        os.setresuid(pg_user.pw_uid, pg_user.pw_uid, pg_user.pw_uid)

    subprocess.run([initdb_cmd, "-D", pg_data], preexec_fn=preexec_pg, check=True, stdout=subprocess.DEVNULL)
    subprocess.run([pg_ctl_cmd, "-D", pg_data, "-o", f"-c listen_addresses= -c unix_socket_directories={pg_sock} -c fsync=off -c synchronous_commit=off -c full_page_writes=off", "start"], preexec_fn=preexec_pg, check=True, stdout=subprocess.DEVNULL)

    wait_for_socket(f"{pg_sock}/.s.PGSQL.5432", "PostgreSQL")

    p = subprocess.Popen([psql_cmd, "-h", pg_sock, "-d", "postgres"], stdin=subprocess.PIPE, preexec_fn=preexec_pg, text=True, stdout=subprocess.DEVNULL)
    sql_create_roles = f"""
    DO $$
    BEGIN
        IF NOT EXISTS (SELECT FROM pg_catalog.pg_roles WHERE rolname = 'odoo') THEN
            CREATE ROLE odoo WITH SUPERUSER LOGIN PASSWORD 'odoo';
        END IF;
        IF NOT EXISTS (SELECT FROM pg_catalog.pg_roles WHERE rolname = '{orig_user}') THEN
            CREATE ROLE {orig_user} WITH SUPERUSER LOGIN;
        END IF;
    END $$;
    """
    p.communicate(sql_create_roles)
    p.wait()

    # 4. Redis Sandboxing
    redis_user = pwd.getpwnam("redis")
    redis_proc = subprocess.Popen(["redis-server", "--daemonize", "no"], preexec_fn=lambda: (os.setresgid(redis_user.pw_gid, redis_user.pw_gid, redis_user.pw_gid), os.setresuid(redis_user.pw_uid, redis_user.pw_uid, redis_user.pw_uid)), stdout=subprocess.DEVNULL)
    wait_for_port(6379, "Redis")

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
        os.setresgid(rmq_user.pw_gid, rmq_user.pw_gid, rmq_user.pw_gid)
        os.setresuid(rmq_user.pw_uid, rmq_user.pw_uid, rmq_user.pw_uid)
        os.environ["HOME"] = "/var/lib/rabbitmq"

    subprocess.run(["rabbitmq-server", "-detached"], preexec_fn=preexec_rmq, check=True, stdout=subprocess.DEVNULL)
    wait_for_port(5672, "RabbitMQ")

    # 6. Execute Inner Odoo Test Suite
    os.environ["PYTHONDONTWRITEBYTECODE"] = "1"
    os.environ["HAMS_ISOLATED_NS"] = "1"
    os.environ["PGHOST"] = pg_sock
    host_tmp_dir = os.environ.get("HAMS_REAL_LOG_DIRECTORY", "/var/tmp")
    os.makedirs(host_tmp_dir, exist_ok=True)
    try:
        os.chmod(host_tmp_dir, 0o1777)
    except OSError:
        pass
    os.environ["ODOO_TEST_CHROME_ARGS"] = f"--headless --no-sandbox --disable-dev-shm-usage --disable-gpu --disable-software-rasterizer --disable-features=ServiceWorker,SharedWorker --user-data-dir={host_tmp_dir} --single-process"
    os.environ["HAMS_REAL_LOG_DIRECTORY"] = real_log_dir
    os.environ["HOME"] = "/var/lib/odoo"
    os.environ["XDG_DATA_HOME"] = "/var/lib/odoo/.local/share"

    odoo_user = pwd.getpwnam("odoo")
    def preexec_odoo():
        os.setresgid(odoo_user.pw_gid, odoo_user.pw_gid, odoo_user.pw_gid)
        os.setresuid(odoo_user.pw_uid, odoo_user.pw_uid, odoo_user.pw_uid)

    test_cmd = [sys.executable, os.path.abspath(__file__)] + sys_args
    ret = subprocess.run(test_cmd, preexec_fn=preexec_odoo).returncode

    # 7. Graceful Ephemeral Teardown
    subprocess.run(["rabbitmqctl", "stop"], preexec_fn=preexec_rmq, check=False, stdout=subprocess.DEVNULL)
    redis_proc.terminate()
    subprocess.run([pg_ctl_cmd, "-D", pg_data, "-m", "fast", "stop"], preexec_fn=preexec_pg, check=False, stdout=subprocess.DEVNULL)

    if os.path.exists("/opt/hams/spool/filtered_test.txt"):
        os.makedirs(real_log_dir, exist_ok=True)
        dest_file = os.path.join(real_log_dir, "filtered_test.txt")
        shutil.copy2("/opt/hams/spool/filtered_test.txt", dest_file)
        orig_uid = pwd.getpwnam(orig_user).pw_uid
        os.chown(dest_file, orig_uid, -1)

    for prof in glob.glob("/opt/hams/spool/*.prof"):
        dst = os.path.join(real_log_dir, os.path.basename(prof))
        shutil.copy2(prof, dst)
        os.chown(dst, orig_uid, -1)

    sys.exit(ret)


def provision_jules(base_dir, already_provisioned=False):
    """Provisions a pre-isolated Jules VM environment"""
    orig_user = os.environ.get("SUDO_USER") or os.environ.get("USER")

    if os.geteuid() != 0:
        print("[*] Elevating privileges (sudo) to provision Jules environment...")
        exec_cmd = ["sudo", "-H", "-E", sys.executable, os.path.abspath(__file__), "--internal-jules-provision"]
        if already_provisioned:
            exec_cmd.append("--already-provisioned")
        subprocess.run(exec_cmd, check=True)
        pg_socket = "/opt/hams/pgsock"
        wait_for_socket(f"{pg_socket}/.s.PGSQL.5432", "PostgreSQL")
        os.environ["PGHOST"] = pg_socket

        def teardown():
            try:
                u_info = pwd.getpwnam(orig_user)
                def pre_td():
                    os.setresgid(u_info.pw_gid, u_info.pw_gid, u_info.pw_gid)
                    os.setresuid(u_info.pw_uid, u_info.pw_uid, u_info.pw_uid)
                pg_ctl_cmd = get_pg_bin("pg_ctl")
                subprocess.run([pg_ctl_cmd, "-D", "/opt/hams/pgdata", "-m", "fast", "stop"], preexec_fn=pre_td, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            except Exception as e: # audit-ignore-catch-all
                _logger.warning("Teardown exception: %s", e)
        atexit.register(teardown)
        return

    env_vars = dict(os.environ)
    env_vars["DEBIAN_FRONTEND"] = "noninteractive"

    def run_sys(cmd, **kw):
        print(f"[*] Running: {' '.join(cmd)}")
        if "env" not in kw:
            kw["env"] = env_vars
        return subprocess.run(cmd, check=True, **kw)

    if not already_provisioned:
        print("[*] Provisioning APT Sources and Packages...")
        try:
            apt_opts = ["-o", "Dpkg::Options::=--force-confdef", "-o", "Dpkg::Options::=--force-confold", "-o", "Dpkg::Lock::Timeout=120"]
            # APT Packages MUST run before static files to ensure python3-setuptools exists for PyPDF2 setup.py
            run_sys(["apt-get", "update"] + apt_opts)
            run_sys(["apt-get", "install", "-y"] + apt_opts + ["python3-setuptools", "python3-stdeb", "dh-python", "python3-all", "fakeroot", "curl", "gnupg", "lsb-release", "python3-pip"])

            run_sys(["bash", "-c", "curl -fsSL https://www.postgresql.org/media/keys/ACCC4CF8.asc | gpg --dearmor --yes -o /usr/share/keyrings/postgresql-keyring.gpg"])
            run_sys(["bash", "-c", "echo \"deb [signed-by=/usr/share/keyrings/postgresql-keyring.gpg] http://apt.postgresql.org/pub/repos/apt/ $(lsb_release -cs)-pgdg main\" > /etc/apt/sources.list.d/pgdg.list"])

            run_sys(["apt-get", "update"] + apt_opts)
            run_sys(["apt-get", "install", "-y"] + apt_opts + ["postgresql-common"])
            run_sys(["apt-get", "install", "-y"] + apt_opts + ["postgresql-client"])

            infrastructure.provision_static_files(run_sys, env_vars, environment="prod")
            infrastructure.provision_apt_packages(run_sys, environment="early_prod")
            run_sys(["apt-get", "install", "-y"] + apt_opts + ["odoo"])

            req_file = os.path.join(base_dir, "requirements.txt")
            if os.path.exists(req_file):
                try:
                    run_sys(["/usr/bin/python3", "-m", "pip", "install", "--break-system-packages", "--ignore-installed", "-r", req_file])
                except subprocess.CalledProcessError as e:
                    print(f"[*] WARNING: pip install encountered an error: {e}. Continuing to ensure database provisioning completes.")

            print("[*] Preparing testing directories with production paths...")
            try:
                odoo_pwnam = pwd.getpwnam("odoo")
                odoo_uid, odoo_gid = odoo_pwnam.pw_uid, odoo_pwnam.pw_gid

                for d in [
                    "/var/lib/odoo/daemon_keys", "/opt/hams/etc/keys", "/opt/hams/spool",
                    "/opt/hams/spool/ncvec", "/opt/hams/spool/adif_queue", "/opt/hams/cache",
                    "/opt/hams/pycache", "/opt/hams/failed_input", "/opt/hams/downloads",
                    "/var/lib/odoo/backups"
                ]:
                    os.makedirs(d, exist_ok=True)
                    try:
                        os.chown(d, odoo_uid, odoo_gid)
                        os.chmod(d, 0o775)
                    except OSError:
                        pass
            except KeyError:
                print("[*] WARNING: 'odoo' user not found during directory preparation.")

        except subprocess.CalledProcessError as e:
            print(f"❌ ERROR: Failed to provision system packages: {e}")
            sys.exit(1)

    print("[*] Clearing port 8069 bindings...")
    subprocess.run(["fuser", "-k", "8069/tcp"], check=False, stderr=subprocess.DEVNULL)

    print("[*] Configuring local PostgreSQL...")
    try:
        initdb_cmd = get_pg_bin("initdb")
        pg_ctl_cmd = get_pg_bin("pg_ctl")
        psql_cmd = get_pg_bin("psql")
    except FileNotFoundError as e:
        print(f"❌ ERROR: {e}")
        sys.exit(1)

    pg_data, pg_socket = "/opt/hams/pgdata", "/opt/hams/pgsock"

    subprocess.run(["systemctl", "stop", "postgresql"], check=False, stderr=subprocess.DEVNULL)

    os.makedirs(pg_data, exist_ok=True)
    os.makedirs(pg_socket, exist_ok=True)

    user_info = pwd.getpwnam(orig_user)
    orig_uid, orig_gid = user_info.pw_uid, user_info.pw_gid

    def preexec_orig_user():
        os.setresgid(orig_gid, orig_gid, orig_gid)
        os.setresuid(orig_uid, orig_uid, orig_uid)

    os.chown(pg_data, orig_uid, orig_gid)
    os.chown(pg_socket, orig_uid, orig_gid)

    os.chmod(pg_data, 0o700)
    os.chmod(pg_socket, 0o2775)

    if not os.listdir(pg_data):
        subprocess.run([initdb_cmd, "-D", pg_data], preexec_fn=preexec_orig_user, check=True)

    subprocess.run([pg_ctl_cmd, "-D", pg_data, "-m", "fast", "stop"], preexec_fn=preexec_orig_user, check=False, stderr=subprocess.DEVNULL)

    try:
        os.remove(f"{pg_data}/postmaster.pid")
    except OSError:
        pass

    subprocess.run([pg_ctl_cmd, "-D", pg_data, "-o", f"-c listen_addresses= -c unix_socket_directories={pg_socket} -c fsync=off -c synchronous_commit=off -c full_page_writes=off", "start"], preexec_fn=preexec_orig_user, check=True)
    wait_for_socket(f"{pg_socket}/.s.PGSQL.5432", "PostgreSQL")

    custom_env = dict(os.environ)
    custom_env["PGUSER"] = orig_user
    p = subprocess.Popen([psql_cmd, "-h", pg_socket, "-d", "postgres"], stdin=subprocess.PIPE, preexec_fn=preexec_orig_user, env=custom_env, text=True, stdout=subprocess.DEVNULL)
    sql_create_roles = f"""
    DO $$
    BEGIN
        IF NOT EXISTS (SELECT FROM pg_catalog.pg_roles WHERE rolname = 'odoo') THEN
            CREATE ROLE odoo WITH SUPERUSER LOGIN PASSWORD 'odoo';
        END IF;
        IF NOT EXISTS (SELECT FROM pg_catalog.pg_roles WHERE rolname = '{orig_user}') THEN
            CREATE ROLE {orig_user} WITH SUPERUSER LOGIN;
        END IF;
    END $$;
    """
    p.communicate(sql_create_roles)
    p.wait()

    print("[*] Starting local Redis and RabbitMQ...")
    subprocess.run(["systemctl", "start", "redis-server"], check=False, stderr=subprocess.DEVNULL)
    subprocess.run(["systemctl", "start", "rabbitmq-server"], check=False, stderr=subprocess.DEVNULL)

    wait_for_port(6379, "Redis")
    wait_for_port(5672, "RabbitMQ")

    os.environ["PGHOST"] = pg_socket


def main():
    os.environ.setdefault("HAMS_KEYS_DIR", "/opt/hams/etc/keys")

    if os.environ.get("HAMS_ISOLATED_NS") != "1" and not os.environ.get("IN_JULES_VM") and not os.environ.get("JULES_SESSION_ID"):
        if "--internal-ns-init" in sys.argv:
            # Phase 2: Execute completely within Python (No bash script interpolation)
            real_log_dir = os.environ.get("HAMS_REAL_LOG_DIRECTORY")
            sys_args = [arg for arg in sys.argv[1:] if arg != "--internal-ns-init"]
            setup_namespace_and_run_tests(real_log_dir, sys_args)
            return

        parser = argparse.ArgumentParser()
        parser.add_argument("-l", "--log-directory", default="~/tmp")
        args, _ = parser.parse_known_args()

        real_log_dir = os.path.abspath(os.path.expanduser(args.log_directory))
        os.makedirs(real_log_dir, exist_ok=True)
        try:
            os.chmod(real_log_dir, 0o1777)
        except OSError:
            pass
        print("[*] Routing test execution to isolated Python namespace...")

        os.environ["HAMS_REAL_LOG_DIRECTORY"] = real_log_dir
        exec_cmd = ["unshare", "-m", sys.executable, os.path.abspath(__file__), "--internal-ns-init"] + sys.argv[1:]

        if os.geteuid() != 0:
            print("[*] Elevating privileges (sudo) to construct isolated mount namespace...")
            exec_cmd = ["sudo", "-H", "-E"] + exec_cmd
            os.execvpe("sudo", exec_cmd, os.environ)
        else:
            # os.execvpe completely replaces the current process, passing control natively
            os.execvpe("unshare", exec_cmd, os.environ)
        return

    os.environ["PYTHONWARNINGS"] = "ignore::DeprecationWarning"
    host_tmp_dir = os.environ.get("HAMS_REAL_LOG_DIRECTORY", "/var/tmp")
    os.makedirs(host_tmp_dir, exist_ok=True)
    try:
        os.chmod(host_tmp_dir, 0o1777)
    except OSError:
        pass
    os.environ.setdefault("ODOO_TEST_CHROME_ARGS", f"--headless --no-sandbox --disable-dev-shm-usage --disable-gpu --disable-software-rasterizer --disable-features=ServiceWorker,SharedWorker,DialMediaRouteProvider --user-data-dir={host_tmp_dir}")
    os.environ.setdefault("DBUS_SESSION_BUS_ADDRESS", "/dev/null")

    # Force system site-packages resolution for Odoo core in restricted venvs
    sys_paths = os.environ.get("PYTHONPATH", "")
    if "/usr/lib/python3/dist-packages" not in sys_paths:
        os.environ["PYTHONPATH"] = f"/usr/lib/python3/dist-packages:{sys_paths}".strip(":")

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
    parser.add_argument("-l", "--log-directory", default="~/tmp")
    parser.add_argument("-c", "--config", default="ignore_list.txt")
    parser.add_argument("--daemon")
    parser.add_argument("--provision-jules", action="store_true")
    parser.add_argument("--already-provisioned", action="store_true")
    parser.add_argument("--profile", action="store_true")
    parser.add_argument("--internal-jules-provision", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()

    is_jules = bool(os.environ.get("IN_JULES_VM")) or bool(os.environ.get("JULES_SESSION_ID"))

    if args.internal_jules_provision:
        provision_jules(base_dir, already_provisioned=args.already_provisioned)
        sys.exit(0)

    venv_python = "/usr/bin/python3" if is_jules else os.path.join(base_dir, ".venv", "bin", "python")
    odoo_bin = "/usr/bin/odoo"
    addons_path = get_addons_path(base_dir)

    ignore_filepath = os.path.join(base_dir, args.config)
    ignore_patterns = load_ignore_file(ignore_filepath)

    target_modules = [m.strip() for m in args.module.split(",")] if args.module else get_local_modules(base_dir, ignore_patterns)

    if not args.module and "caching" in target_modules:
        target_modules.remove("caching")

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

    if is_jules:
        if args.provision_jules and not args.module:
            print("[*] Provisioning Jules VM without running global tests to prevent timeouts. Use -u to test specific modules.")
            provision_jules(base_dir, already_provisioned=False)
            print("[*] Provisioning sequence completed successfully.")
            sys.exit(0)
        elif args.provision_jules:
            provision_jules(base_dir, already_provisioned=False)
        elif args.already_provisioned:
            provision_jules(base_dir, already_provisioned=True)
        else:
            if os.path.exists("/opt/hams/pgdata/PG_VERSION") or shutil.which("psql"):
                provision_jules(base_dir, already_provisioned=True)
            else:
                print("[*] Jules VM detected without provisioning flags. Auto-provisioning...")
                provision_jules(base_dir, already_provisioned=False)

    extractor = FailureExtractor(args.log_directory)
    print(f"==========================================================\n 🧪 ODOO TEST RUNNER [{args.mode.upper()} MODE]\n==========================================================")

    check_linters(venv_python, base_dir, ignore_filepath, extractor, target_modules)

    final_rc = 0

    if args.mode in ("standard", "integration"):
        rebuild_db(args.db)

        # Inject environment variables for daemons spawned securely by tests
        os.environ["ODOO_URL"] = "http://127.0.0.1:8069"
        os.environ["DB_NAME"] = args.db
        os.environ["ODOO_USER"] = "admin"
        os.environ["ODOO_PASSWORD"] = "admin"

        cmd = get_odoo_test_cmd() + [
            odoo_bin, "--load=base,web,hams_test", "--addons-path", addons_path,
            "--dev=all", "-d", args.db, "-i", mod_string, "--test-enable",
            "--test-tags", test_tags, "--stop-after-init", "--workers=0",
            "--max-cron-threads=0", "--http-interface", "127.0.0.1", "--http-port", "8069"
        ]

        rc_odoo = run_cmd(cmd, extractor)
        if rc_odoo != 0:
            final_rc = rc_odoo

    elif args.mode == "individual":
        os.environ["ODOO_URL"] = "http://127.0.0.1:8069"
        os.environ["DB_NAME"] = args.db
        os.environ["ODOO_USER"] = "admin"
        os.environ["ODOO_PASSWORD"] = "admin"

        for mod in target_modules:
            rebuild_db(args.db)
            rc = run_cmd(get_odoo_test_cmd(f"_{mod}") + [
                odoo_bin, "--load=base,web,hams_test", "--addons-path", addons_path,
                "--dev=all", "-d", args.db, "-i", mod, "--test-enable",
                "--test-tags", f"/{mod}", "--stop-after-init", "--workers=0",
                "--max-cron-threads=0", "--http-interface", "127.0.0.1", "--http-port", "8069"
            ], extractor)
            if rc != 0: final_rc = 1

    sys.exit(final_rc)

if __name__ == "__main__":
    main()
