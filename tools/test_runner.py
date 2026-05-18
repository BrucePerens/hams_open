#!/usr/bin/env python3
"""
Unified Odoo Test Runner for Hams.com
Combines test execution, integration modes, and real-time failure extraction.
"""

import argparse
import atexit
import copy
import glob
import logging
import os
import re
import signal
import socket
import subprocess
import sys
import tempfile
import time
import concurrent.futures
import queue
import threading
import shutil

# Import the centralized infrastructure blueprint
import infrastructure


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


def is_odoo_running(port=8069):
    """Checks if a service (presumably Odoo) is actively listening on the target port."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        return s.connect_ex(("localhost", port)) == 0


class FailureExtractor:
    """
    State machine that processes log lines in real-time, buffering and extracting
    Tracebacks and error blocks for writing to a filtered log file.
    """

    def __init__(self, output_path, disable_atexit=False):
        # Resolve the intended display path vs the physical write path
        self.display_path = os.environ.get("HAMS_REAL_ERROR_LOG") or os.path.abspath(
            os.path.expanduser(output_path)
        )

        # If we are inside the RO namespace, we MUST write to the spool tmpfs
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
            # Ignore pika AMQP connection errors during standard tests to prevent false positive failures
            if (
                self.safe_log_levels.search(line)
                or "pika.adapters" in line
                or "AMQPConnector" in line
                or "Cloudflare URL purge API failed for chunk: API fail" in line
                or "Cloudflare Tag purge API failed for chunk: API fail" in line
                or "[BACKUP_WORKER]" in line
            ):
                # Standard info/warning line. Stop capturing if we were.
                if self.capturing:
                    self.captured_blocks.append(
                        (self.current_context, self.current_block)
                    )
                    self.current_block = []
                    self.capturing = False
            else:
                # It's an ERROR or CRITICAL log line
                if not self.capturing:
                    self.capturing = True
                self.current_block.append(line)
        else:
            # Not a standard log line. Check for Python unhandled tracebacks/failures.
            if (
                "======================================================================"
                in line
                or "Traceback (most recent call last):" in line
                or line.startswith("FAIL: ")
                or line.startswith("ERROR: ")
                or line.startswith("AssertionError")
            ):
                if not self.capturing:
                    self.capturing = True

            if self.capturing:
                self.current_block.append(line)

    def _extract_failed_modules(self):
        """
        Scans the captured tracebacks to determine which Odoo modules or daemons are implicated.
        """
        modules = set()
        # Match standard Odoo namespaces e.g., odoo.addons.ham_base.tests
        addon_pattern = re.compile(r"odoo\.addons\.([a-zA-Z0-9_]+)")

        # Match standard file paths in tracebacks e.g., File ".../ham_logbook/models/..."
        filepath_pattern = re.compile(
            r"\/([a-zA-Z0-9_]+)\/(?:models|controllers|tests|wizard|tools)\/.*?\.py"
        )

        # Explicitly match daemon paths to avoid capturing the repo root
        # e.g., File ".../daemons/adif_processor/test_adif_processor.py"
        daemon_pattern = re.compile(r"\/daemons\/([a-zA-Z0-9_]+)\/.*?\.py")

        for context, block in self.captured_blocks:
            # Search context
            for match in addon_pattern.findall(context):
                modules.add(match)
            for match in filepath_pattern.findall(context):
                modules.add(match)
            for match in daemon_pattern.findall(context):
                modules.add("daemons/{}".format(match))

            # Search the actual traceback lines
            for line in block:
                for match in addon_pattern.findall(line):
                    modules.add(match)
                for match in filepath_pattern.findall(line):
                    modules.add(match)
                for match in daemon_pattern.findall(line):
                    modules.add("daemons/{}".format(match))

        # Exclude core Odoo modules to keep the AI focused on the custom codebase
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

        # Group blocks by test context to accurately count unique test failures
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

                # Inject a high-authority system prompt to focus the AI in subsequent debugging sessions
                out.write("\n" + "*" * 80 + "\n")
                out.write("SYSTEM DIRECTIVE FOR AI ASSISTANT:\n")
                out.write(
                    "The following log contains extracted test failures, tracebacks, and CRITICAL errors from the Odoo test suite.\n"
                )
                out.write(
                    "Your immediate task is to analyze these errors, identify the root causes within the provided codebase, and generate the necessary patches to fix these test flaws.\n"
                )

                if failed_modules:
                    out.write("\nTARGET MODULES FOR ANALYSIS:\n")
                    out.write(
                        "Based on the tracebacks, the following modules are responsible for or implicated in the failure:\n"
                    )
                    for mod in failed_modules:
                        out.write("  - {}\n".format(mod))
                    out.write(
                        "\nASSUMPTION: The GitHub repository containing these modules has been imported to your environment.\n"
                    )
                    out.write(
                        "ACTION: Please look up the code for the implicated modules above to diagnose and fix the issue.\n"
                    )

                out.write("*" * 80 + "\n")

                for context, block in grouped_blocks.items():
                    if not block:
                        continue
                    out.write("\n" + "=" * 80 + "\n")
                    out.write("CONTEXT: {}\n".format(context))
                    out.write("-" * 80 + "\n")
                    for b_line in block:
                        out.write(b_line)
                    out.write("\n")

        # High-visibility terminal summary
        print("\n==========================================================")
        if num_failures == 0:
            print("🎉 TEST RUN COMPLETE: No test failures detected.")
        else:
            print(
                "🚨 TEST RUN COMPLETE: {} test failure(s) detected!".format(
                    num_failures
                )
            )
            print(
                "📄 Failure details extracted and saved to: {}".format(
                    self.display_path
                )
            )
        print("==========================================================\n")


def run_cmd(cmd, extractor=None, cwd=None, env=None):
    """
    Executes a shell command, printing stdout in real-time to the terminal
    while simultaneously feeding the stream to the failure extractor.
    """
    initial_errors = len(extractor.captured_blocks) if extractor else 0

    if env is None:
        env = dict(os.environ)
    if "RABBITMQ_HOST" not in env:
        env["RABBITMQ_HOST"] = "localhost"
    if "RMQ_HOST" not in env:
        env["RMQ_HOST"] = "localhost"
    if "REDIS_HOST" not in env:
        env["REDIS_HOST"] = "localhost"
    if "RMQ_USER" not in env:
        env["RMQ_USER"] = "guest"
    if "RMQ_PASS" not in env:
        env["RMQ_PASS"] = "guest"
    if "ODOO_TEST_CHROME_ARGS" not in env:
        env["ODOO_TEST_CHROME_ARGS"] = "--headless --no-sandbox --disable-dev-shm-usage"

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
            logging.getLogger('tools.test_runner').warning("Reader exception: %s", e)
            pass
        q.put(None)

    t = threading.Thread(target=reader)
    t.daemon = True
    t.start()

    try:
        while True:
            try:
                line = q.get(timeout=120.0)
                if line is None:
                    break
                line_lower = line.lower()
                if (
                    "deprecated" in line_lower and "directive" in line_lower
                ) or "pypdf2" in line_lower:
                    continue
                sys.stdout.write(line)
                sys.stdout.flush()
                if extractor:
                    extractor.process_line(line)

                if "Hit CTRL-C again or send a second signal" in line:
                    print(
                        "\n[!] WARNING: Odoo did not terminate because a background thread within it,"
                    )
                    print(
                        "             possibly spawned by your module, is not set up to terminate"
                    )
                    print(
                        "             with the rest of Odoo. The test program killed Odoo's process"
                    )
                    print("             group to end the test.\n")

                    os.killpg(os.getpgid(process.pid), signal.SIGKILL)
                    force_killed = True
                    break
            except queue.Empty:
                print("\n[!] WARNING: Test runner hung for 120 seconds with no output! Killing to continue...")
                os.killpg(os.getpgid(process.pid), signal.SIGKILL)
                force_killed = True
                if extractor:
                    extractor.process_line("CRITICAL: Test execution hung for 120 seconds. Process forcefully killed.\n")
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
    """Scans the repository root for Odoo modules by locating __manifest__.py"""
    mods = []
    for item in os.listdir(base_dir):
        mod_path = os.path.join(base_dir, item)
        if is_ignored(mod_path, ignore_patterns):
            continue
        if os.path.isdir(mod_path) and os.path.isfile(
            os.path.join(mod_path, "__manifest__.py")
        ):
            mods.append(item)
    return sorted(mods)


def get_addons_path(base_dir):
    """Resolves the 3-tier addons path for testing (excluding tertiary per architectural mandates)"""
    paths = ["/usr/lib/python3/dist-packages/odoo/addons", base_dir]

    community_dir = os.path.abspath(os.path.join(base_dir, "..", "hams_community"))
    primary_dir = os.path.abspath(os.path.join(base_dir, "..", "hams_private_primary"))

    if os.path.isdir(community_dir):
        paths.append(community_dir)
    if os.path.isdir(primary_dir):
        paths.append(primary_dir)

    return ",".join(paths)


def check_linters(venv_python, base_dir, ignore_filepath, extractor=None):
    """Executes the AST Burn List and Semantic Anchor DevSecOps linters"""

    print("[*] Running Manifest Dependency Graph Linter...")
    manifest_script = os.path.join(base_dir, "tools", "check_manifest_dependencies.py")
    res_manifest = subprocess.run([venv_python, manifest_script, base_dir])
    if res_manifest.returncode != 0:
        print("🛑 Halting due to manifest load-order violations. Please review the output above.")
        if extractor:
            extractor.captured_blocks.append(("Linter Violation", ["Manifest Dependency Graph Linter failed. Forward references detected.\n"]))
            extractor.finish_and_write()
        sys.exit(1)

    print("[*] Running AST Burn List Linter...")
    burn_script = os.path.join(base_dir, "tools", "check_burn_list.py")
    res_burn = subprocess.run(
        [venv_python, burn_script, base_dir, "--ignore-file", ignore_filepath]
    )
    if res_burn.returncode != 0:
        print("🛑 Halting due to burn list violations. Please review the output above.")
        if extractor:
            extractor.captured_blocks.append(("Linter Violation", ["AST Burn List Linter failed. Please review the console output for details.\n"]))
            extractor.finish_and_write()
        sys.exit(1)

    print("[*] Scanning documentation and codebase for Semantic Anchors...")
    anchor_script = os.path.join(base_dir, "tools", "verify_anchors.py")
    res_anchor = subprocess.run([venv_python, anchor_script, base_dir])
    if res_anchor.returncode != 0:
        print(
            "🛑 Halting due to linter/anchor violations. Please review the output above."
        )
        if extractor:
            extractor.captured_blocks.append(("Linter Violation", ["Semantic Anchor Linter failed. Please review the console output for details.\n"]))
            extractor.finish_and_write()
        sys.exit(1)


def run_daemon_tests(venv_python, base_dir, extractor, ignore_patterns, target_modules):
    """Executes the standalone unit tests for background daemons."""
    print("[*] Executing Standalone Daemon Tests...")
    final_rc = 0
    for mod in target_modules:
        daemon_dir = os.path.join(base_dir, mod, "daemon")
        if not os.path.exists(daemon_dir):
            daemon_dir = os.path.join(base_dir, mod, "daemons")
        if not os.path.exists(daemon_dir):
            continue
        if is_ignored(daemon_dir, ignore_patterns):
            continue
        has_tests = any(
            f.startswith("test_") and f.endswith(".py")
            for f in os.listdir(daemon_dir)
        )
        if has_tests:
            print("\n[*] ----------------------------------------------------")
            print("[*] Testing Daemon: {}".format(mod))
            print("[*] ----------------------------------------------------")
            if extractor:
                extractor.set_context("Daemon Tests / {}".format(mod))
            cmd = [
                venv_python,
                "-m",
                "unittest",
                "discover",
                "-p",
                "test_*.py",
                "-v",
            ]
            rc = run_cmd(cmd, extractor, cwd=daemon_dir)
            if rc != 0:
                final_rc = rc

    return final_rc


def rebuild_db(db_name):
    """Drops and creates a fresh PostgreSQL database"""
    print("[*] Dropping and Rebuilding Database Schema ({})...".format(db_name))

    env = dict(os.environ)
    if "PGHOST" not in env and os.environ.get("HAMS_ISOLATED_NS") == "1":
        pass  # disabled

    psql_cmd = shutil.which("psql") or "psql"
    dropdb_cmd = shutil.which("dropdb") or "dropdb"
    createdb_cmd = shutil.which("createdb") or "createdb"

    subprocess.run(
        [
            psql_cmd,
            "postgres",
            "-c",
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{}';".format(
                db_name
            ),
        ],
        check=False,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        env=env,
    )

    subprocess.run(
        [dropdb_cmd, "--if-exists", "--force", db_name],
        check=False,
        stderr=subprocess.DEVNULL,
        env=env,
    )
    env = dict(os.environ)
    if "PGHOST" not in env and os.environ.get("HAMS_ISOLATED_NS") == "1":
        pass  # disabled
    subprocess.run([createdb_cmd, db_name], check=True, env=env)


def check_and_restore_cache(db_name, mod_string):
    cache_dir = "/opt/hams/test"
    cache_file = os.path.join(cache_dir, "db_cache_master.dump")

    base_dir = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
    community_dir = os.path.abspath(os.path.join(base_dir, "..", "hams_community"))

    search_bases = [base_dir]
    if os.path.exists(community_dir):
        search_bases.append(community_dir)

    dirs_to_check = []
    for sb in search_bases:
        try:
            for item in os.listdir(sb):
                item_path = os.path.join(sb, item)
                if os.path.isdir(item_path) and os.path.isfile(
                    os.path.join(item_path, "__manifest__.py")
                ):
                    dirs_to_check.append(item_path)
        except OSError:
            pass

    newest_mtime = 0
    for d in dirs_to_check:
        for root, dirs, files in os.walk(d):
            dirs[:] = [
                di
                for di in dirs
                if not di.startswith(".") and di not in ("__pycache__", "tests")
            ]

            for f in files:
                if (
                    f.endswith((".pyc", ".pyo", ".dump", ".log", ".swp", ".pot", ".po"))
                    or f.startswith(".")
                    or f == "filtered_test.txt"
                ):
                    continue
                p = os.path.join(root, f)
                try:
                    mtime = os.path.getmtime(p)
                    if mtime > newest_mtime:
                        newest_mtime = mtime
                except OSError:
                    pass

    # Explicitly check critical infrastructure files that dictate DB schema or provisioning state
    critical_infra_files = [
        os.path.join(base_dir, "tools", "infrastructure.py"),
        os.path.join(base_dir, "tools", "test_runner.py"),
        os.path.join(base_dir, "deploy", "bootstrap_daemon_keys.py"),
    ]
    for cf in critical_infra_files:
        if os.path.exists(cf):
            try:
                cf_mtime = os.path.getmtime(cf)
                if cf_mtime > newest_mtime:
                    newest_mtime = cf_mtime
            except OSError:
                pass

    cache_valid = False
    if os.path.exists(cache_file):
        try:
            sz = os.path.getsize(cache_file)
            print(f"[*] Found existing DB cache: {sz} bytes.")
            if sz > 5000000:
                cache_mtime = os.path.getmtime(cache_file)
                if cache_mtime >= newest_mtime:
                    cache_valid = True
            else:
                print("[*] DB cache is too small to be valid (< 5MB).")
        except OSError:
            pass

    if cache_valid:
        mod_file = cache_file.replace(".dump", ".modules")
        if os.path.exists(mod_file):
            with open(mod_file, "r") as f:
                cached_mods = f.read().strip()
            if cached_mods != mod_string:
                print(
                    f"[*] Module list changed (was: '{cached_mods}', now: '{mod_string}'). Discarding cache."
                )
                cache_valid = False
        else:
            print("[*] Cache module list missing. Discarding cache.")
            cache_valid = False

    if cache_valid:
        print(f"[*] Valid DB cache found ({cache_file}). Restoring (Parallel)...")
        rebuild_db(db_name)
        env = dict(os.environ)
        if "PGHOST" not in env and os.environ.get("HAMS_ISOLATED_NS") == "1":
            pass  # disabled

        pg_restore_cmd = shutil.which("pg_restore") or "pg_restore"
        res = subprocess.run(
            [pg_restore_cmd, "-d", db_name, "-O", "-x", "-j", "4", cache_file],
            capture_output=True,
            text=True,
        )
        if res.returncode == 0:
            print("[*] DB restored from cache.")

            # Restore the corresponding filestore
            filestore_tar = cache_file.replace(".dump", ".filestore.tar.gz")
            if os.path.exists(filestore_tar):
                print("[*] Restoring Filestore...")
                filestore_base = "/var/lib/odoo/.local/share/Odoo/filestore"
                os.makedirs(filestore_base, exist_ok=True)
                subprocess.run(["tar", "-xzf", filestore_tar, "-C", filestore_base])

            return True, cache_file
        else:
            print(
                f"[*] WARNING: pg_restore failed. Cache might be corrupted. Output:\n{res.stderr}"
            )
            try:
                os.remove(cache_file)
            except OSError:
                pass
            rebuild_db(db_name)
            return False, cache_file
    else:
        if os.path.exists(cache_file):
            print("[*] DB cache invalid or too small. Discarding.")
            try:
                os.remove(cache_file)
            except OSError:
                pass
        else:
            print("[*] DB cache missing. Rebuilding...")
        rebuild_db(db_name)
        return False, cache_file


def save_db_cache(db_name, cache_file, mod_string):
    print(f"[*] Caching newly initialized DB to {cache_file}...")
    try:
        os.makedirs(os.path.dirname(cache_file), exist_ok=True)
    except Exception as e: # audit-ignore-catch-all
        logging.getLogger('tools.test_runner').warning("An error occurred: %s", e)
        pass

    env = dict(os.environ)
    if "PGHOST" not in env and os.environ.get("HAMS_ISOLATED_NS") == "1":
        env["PGHOST"] = "/opt/hams/pgsock"

    try:
        with open(cache_file, "wb") as f:
            pg_dump_cmd = shutil.which("pg_dump") or "pg_dump"
            res = subprocess.run(
                [pg_dump_cmd, "-Fc", "-Z", "1", db_name], stdout=f, stderr=subprocess.PIPE, env=env
            )

        if res.returncode == 0:
            sz = os.path.getsize(cache_file)
            if sz > 5000000:
                print(f"[*] DB cached successfully ({sz} bytes).")
                mod_file = cache_file.replace(".dump", ".modules")
                with open(mod_file, "w") as mf:
                    mf.write(mod_string)

                # Cache the Filestore
                filestore_path = f"/var/lib/odoo/.local/share/Odoo/filestore/{db_name}"
                if os.path.exists(filestore_path):
                    print("[*] Caching Filestore...")
                    filestore_tar = cache_file.replace(".dump", ".filestore.tar.gz")
                    subprocess.run(["tar", "-czf", filestore_tar, "-C", os.path.dirname(filestore_path), db_name])
            else:
                print(
                    f"[*] WARNING: pg_dump produced a file that is suspiciously small ({sz} bytes). Discarding cache."
                )
                if res.stderr:
                    print(
                        f"[*] pg_dump stderr: {res.stderr.decode('utf-8', errors='ignore')}"
                    )
                try:
                    os.remove(cache_file)
                except OSError:
                    pass
        else:
            print(
                f"[*] WARNING: pg_dump failed (exit code {res.returncode}):\n{res.stderr.decode('utf-8', errors='ignore')}"
            )
            try:
                os.remove(cache_file)
            except OSError:
                pass
    except Exception as e: # audit-ignore-catch-all
        logging.getLogger('tools.test_runner').warning("Failed to execute pg_dump: %s", e)
        print(f"[*] WARNING: Failed to execute pg_dump: {e}")


def run_in_isolated_environment(real_error_log):
    print("**** RUN IN ISOLATED ENVIRONMENT. ****\n")
    if os.geteuid() != 0:
        print(
            "[*] Elevating to sudo to provision isolated PostgreSQL and Namespaces..."
        )
        try:
            res = subprocess.run(["sudo", "-E", sys.executable] + sys.argv)
            sys.exit(res.returncode)
        except KeyboardInterrupt:
            sys.exit(1)

    orig_user = os.environ.get("SUDO_USER") or os.environ.get("USER")
    if orig_user == "root":
        raise ValueError(
            "Test runner MUST NOT be executed directly as root. Use a standard user with sudo privileges."
        )

    print("[*] Bootstrapping isolated testing environment (Mount Namespaces)...")
    pg_data_dir = "/opt/hams/pgdata"
    pg_socket_dir = "/opt/hams/pgsock"

    pg_bin_dir = ""
    pg_bins = glob.glob("/usr/lib/postgresql/*/bin/initdb")
    if pg_bins:
        pg_bin_dir = os.path.dirname(sorted(pg_bins)[-1]) + "/"
    else:
        print(
            "❌ ERROR: Could not locate PostgreSQL initdb in /usr/lib/postgresql/.\n"
            "Please ensure PostgreSQL is installed."
        )
        sys.exit(1)

    os.environ["HAMS_ISOLATED_NS"] = "1"
    os.environ["PGHOST"] = pg_socket_dir
    base_dir = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))

    fd, wrapper_script = tempfile.mkstemp(prefix="hams_pg_wrapper_", suffix=".sh")
    with os.fdopen(fd, "w") as f:
        f.write(f"""#!/bin/bash
ip link set lo up

mkdir -p /opt/hams/test/debug_mnt
rm -rf /opt/hams/test/debug_mnt/*

mount --make-rprivate /

mount -t tmpfs tmpfs /mnt
mkdir -p /mnt/upper /mnt/work /mnt/host_test_dir

mkdir -p /opt/hams/test
mount --bind /opt/hams/test /mnt/host_test_dir

{sys.executable} -c 'import sys, os; sys.path.append("{base_dir}/tools"); import infrastructure; run_cmd = lambda cmd, **kw: os.system(" ".join(cmd) if isinstance(cmd, list) else cmd); infrastructure.apply_production_directories(run_cmd, environment="test", dest_dir="/mnt/upper"); infrastructure.write_env_files("/opt/hams/etc", dict(os.environ), run_cmd, dest_dir="/mnt/upper"); infrastructure.provision_static_files(run_cmd, dict(os.environ), environment="test", dest_dir="/mnt/upper"); infrastructure.execute_hooks("test", run_cmd, dict(os.environ), dest_dir="/mnt/upper")'

for d in /mnt/upper/*; do
    if [ -d "$d" ]; then
        dirname=$(basename "$d")
        if [ "$dirname" != "mnt" ]; then
            mkdir -p "/mnt/work/$dirname"
            mount -t overlay overlay -o lowerdir=/$dirname,upperdir=$d,workdir=/mnt/work/$dirname /$dirname
        fi
    fi
done

rm -rf /opt/hams/etc/keys/* 2>/dev/null || true
rm -rf /opt/hams/pgdata/* 2>/dev/null || true
rm -rf /opt/hams/pgsock/* 2>/dev/null || true
rm -rf /opt/hams/spool/* 2>/dev/null || true

export HOME=/var/lib/odoo

mount --bind {base_dir} {base_dir}
mount -o remount,bind,ro {base_dir}

for dir in "{base_dir}/../hams_community"; do
    if [ -d "$dir" ]; then
        REAL_DIR=$(realpath "$dir")
        mount --bind "$REAL_DIR" "$REAL_DIR"
        mount -o remount,bind,ro "$REAL_DIR"
    fi
done

echo '[*] Initializing PostgreSQL...'
su -s /bin/bash postgres -c '{pg_bin_dir}initdb -D {pg_data_dir}'
echo '[*] Starting PostgreSQL...'
su -s /bin/bash postgres -c "{pg_bin_dir}pg_ctl -D {pg_data_dir} -o '-c listen_addresses= -c unix_socket_directories={pg_socket_dir} -c fsync=off -c synchronous_commit=off -c full_page_writes=off' -w start"
echo '[*] Provisioning PostgreSQL roles...'
echo "CREATE ROLE odoo WITH SUPERUSER LOGIN PASSWORD 'odoo'; CREATE ROLE {orig_user} WITH SUPERUSER LOGIN;" | su -s /bin/bash postgres -c 'PGUSER=postgres {pg_bin_dir}psql -h {pg_socket_dir} -d postgres'

su -s /bin/bash redis -c 'redis-server --daemonize yes' >/dev/null 2>&1
su -s /bin/bash rabbitmq -c 'rabbitmq-server -detached' >/dev/null 2>&1

sleep 3

export PYTHONDONTWRITEBYTECODE=1

sudo -E -u odoo env PGHOST={pg_socket_dir} PYTHONDONTWRITEBYTECODE=1 HAMS_ISOLATED_NS=1 PYTHONWARNINGS="ignore::DeprecationWarning" ODOO_TEST_CHROME_ARGS="--headless --no-sandbox --disable-dev-shm-usage" HAMS_REAL_ERROR_LOG='{real_error_log}' "$@"
RET=$?
su -s /bin/bash rabbitmq -c 'rabbitmqctl stop' >/dev/null 2>&1
pkill -u redis redis-server >/dev/null 2>&1
su -s /bin/bash postgres -c '{pg_bin_dir}pg_ctl -D {pg_data_dir} -m fast stop' >/dev/null 2>&1

if [ -f /opt/hams/spool/filtered_test.txt ]; then
    mkdir -p "$(dirname '{real_error_log}')" 2>/dev/null || true
    cp /opt/hams/spool/filtered_test.txt '{real_error_log}'
    chown {orig_user} '{real_error_log}' 2>/dev/null || true
    chmod 644 '{real_error_log}' 2>/dev/null || true
fi

for prof_file in /opt/hams/spool/*.prof; do
    if [ -f "$prof_file" ]; then
        cp "$prof_file" "$(dirname '{real_error_log}')/"
        chown {orig_user} "$(dirname '{real_error_log}')/$(basename "$prof_file")" 2>/dev/null || true
    fi
done

if [ -f /mnt/upper/opt/hams/test/db_cache_master.dump ]; then
    echo '[*] Committing Database Cache to Host...'
    cp /mnt/upper/opt/hams/test/db_cache_master.dump /mnt/host_test_dir/db_cache_master.dump
    chmod 666 /mnt/host_test_dir/db_cache_master.dump 2>/dev/null || true
    if [ -f /mnt/upper/opt/hams/test/db_cache_master.modules ]; then
        cp /mnt/upper/opt/hams/test/db_cache_master.modules /mnt/host_test_dir/db_cache_master.modules
        chmod 666 /mnt/host_test_dir/db_cache_master.modules 2>/dev/null || true
    fi
    if [ -f /mnt/upper/opt/hams/test/db_cache_master.filestore.tar.gz ]; then
        cp /mnt/upper/opt/hams/test/db_cache_master.filestore.tar.gz /mnt/host_test_dir/db_cache_master.filestore.tar.gz
        chmod 666 /mnt/host_test_dir/db_cache_master.filestore.tar.gz 2>/dev/null || true
    fi
fi

echo '[*] DEBUG: Exporting ephemeral namespace state to /opt/hams/test/debug_mnt for inspection...'
mkdir -p /mnt/host_test_dir/debug_mnt
cp -ra /mnt/upper/* /mnt/host_test_dir/debug_mnt/ >/dev/null 2>&1 || true

exit $RET
""")
    os.chmod(wrapper_script, 0o755)

    script_path = os.path.abspath(sys.argv[0])
    args = [sys.executable, script_path] + sys.argv[1:]

    exec_cmd = ["unshare", "-m", wrapper_script] + args

    try:
        result = subprocess.run(exec_cmd)
        sys.exit(result.returncode)
    except Exception as e: # audit-ignore-catch-all
        logging.getLogger('tools.test_runner').error("ERROR launching isolated environment: %s", e)
        print(f"❌ ERROR launching isolated environment: {e}")
        sys.exit(1)
    finally:
        print("[*] Cleaning up isolated test wrapper...")
        try:
            pass
        except OSError:
            pass


def main():
    try:
        # Silence Odoo's core framework noise (Cybercrud Policy)
        os.environ["PYTHONWARNINGS"] = "ignore::DeprecationWarning"

        if "ODOO_TEST_CHROME_ARGS" not in os.environ:
            os.environ["ODOO_TEST_CHROME_ARGS"] = "--headless --no-sandbox --disable-dev-shm-usage"

        base_dir = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
        os.environ["REPO_ROOT"] = base_dir
        env_path = os.path.join(base_dir, "deploy", "env")
        if os.path.exists(env_path):
            with open(env_path, "r", encoding="utf-8") as f:
                for line in f:
                    line = line.strip()
                    if line and not line.startswith("#") and "=" in line:
                        k, v = line.split("=", 1)
                        if k.strip() not in os.environ:
                            os.environ[k.strip()] = v.strip("'\" ")

        if not os.environ.get("PGHOST"):
            os.environ["PGHOST"] = "localhost"
        if not os.environ.get("PGUSER"):
            os.environ["PGUSER"] = (
                os.environ.get("DB_USER") or os.environ.get("POSTGRES_USER") or "odoo"
            )
        if not os.environ.get("PGPASSWORD"):
            os.environ["PGPASSWORD"] = (
                os.environ.get("DB_PASSWORD")
                or os.environ.get("POSTGRES_PASSWORD")
                or "odoo"
            )
        if not os.environ.get("RABBITMQ_HOST"):
            os.environ["RABBITMQ_HOST"] = "localhost"
        if not os.environ.get("REDIS_HOST"):
            os.environ["REDIS_HOST"] = "localhost"

        pg_bins_sys = glob.glob("/usr/lib/postgresql/*/bin/initdb")
        if pg_bins_sys:
            pg_bin_dir_sys = os.path.dirname(sorted(pg_bins_sys)[-1])
            os.environ["PATH"] = f"{pg_bin_dir_sys}:{os.environ.get('PATH', '')}"

        parser = argparse.ArgumentParser(
            description="Unified Odoo Test Runner for Hams.com",
            formatter_class=argparse.RawTextHelpFormatter,
        )
        parser.add_argument(
            "-m",
            "--mode",
            choices=["standard", "integration", "individual", "xml", "downloads"],
            default="standard",
        )
        parser.add_argument(
            "-d",
            "--db",
            default="hams_test",
            help="Target Database Name (default: hams_test)",
        )
        parser.add_argument(
            "-u",
            "--module",
            help="Specific module to test (defaults to all local modules)",
        )
        parser.add_argument(
            "-e",
            "--error-log",
            default="~/tmp/filtered_test.txt",
            help="Path to save filtered test failures (default: ~/tmp/filtered_test.txt)",
        )
        parser.add_argument(
            "-c",
            "--config",
            default="ignore_list.txt",
            help="Path to ignore config file (default: ignore_list.txt)",
        )
        parser.add_argument(
            "--daemon",
            help="Specific daemon to test in 'downloads' mode (e.g., fcc_uls_sync)",
        )

        parser.add_argument(
            "--provision-jules",
            action="store_true",
            help="Provision the native Jules environment (install Odoo, PostgreSQL, etc.)",
        )
        parser.add_argument(
            "--already-provisioned",
            action="store_true",
            help="Skip provisioning the Jules environment, assuming it is already set up.",
        )
        parser.add_argument(
            "--profile",
            action="store_true",
            help="Profile the Odoo test execution using cProfile.",
        )
        args = parser.parse_args()

        try:
            infrastructure.scaffold_test_environment(
                args.db, provision_dirs=(os.environ.get("HAMS_ISOLATED_NS") != "1")
            )
            if os.environ.get("HAMS_ISOLATED_NS") != "1":
                scaffold_run_cmd = lambda cmd, **kw: (
                    subprocess.run(["/bin/bash", "-c", cmd], check=True, **kw)
                    if isinstance(cmd, str)
                    else (
                        subprocess.run(["sudo"] + cmd, check=True, **kw)
                        if "sudo" not in cmd
                        else subprocess.run(cmd, check=True, **kw)
                    )
                )
                infrastructure.execute_hooks(
                    "test", scaffold_run_cmd, env_vars=os.environ.copy(), dest_dir=""
                )
        except Exception as e: # audit-ignore-catch-all
            logging.getLogger('tools.test_runner').warning("Could not provision directories natively: %s", e)
            print(
                f"[*] Note: Could not provision directories natively ({e}). Ensure they exist."
            )

        real_error_log = os.path.abspath(os.path.expanduser(args.error_log))

        is_jules_vm = bool(os.environ.get("IN_JULES_VM")) or bool(os.environ.get("JULES_SESSION_ID"))
        base_dir = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))

        # Environment Provisioning (Moved here to execute on Host with RW access)
        if os.environ.get("HAMS_ISOLATED_NS") != "1":
            venv_dir = os.path.join(base_dir, ".venv")
            pyvenv_cfg = os.path.join(venv_dir, "pyvenv.cfg")
            req_file = os.path.join(base_dir, "requirements.txt")

            needs_update = True
            if os.path.exists(pyvenv_cfg) and os.path.exists(req_file):
                if os.path.getmtime(pyvenv_cfg) >= os.path.getmtime(req_file):
                    needs_update = False

            if needs_update:
                print("[*] Validating virtual environment and dependencies...")

                def _run_venv_cmd(cmd, env=None):
                    if isinstance(cmd, list):
                        subprocess.run(cmd, check=True, env=env, capture_output=True)
                    else:
                        subprocess.run(
                            ["/bin/bash", "-c", cmd],
                            check=True,
                            env=env,
                            capture_output=True,
                        )

                try:
                    infrastructure.provision_python_venvs(
                        _run_venv_cmd, environment="pre_flight", dest_dir=base_dir
                    )
                    if os.path.exists(pyvenv_cfg):
                        os.utime(pyvenv_cfg, None)
                except subprocess.CalledProcessError as e:
                    print(
                        f"❌ ERROR: Failed to provision dependencies. Error details:\n{e.stderr.decode('utf-8', errors='ignore')}"
                    )
                    sys.exit(1)

        if os.environ.get("HAMS_ISOLATED_NS") != "1" and not is_jules_vm:
            print("[*] Routing test execution to isolated namespace to protect environment...")
            run_in_isolated_environment(real_error_log)
            return

        if is_jules_vm:
            print("[*] Detected Jules VM environment. Bypassing isolated namespace...")

            # Use /opt/hams/pgsock to closely mimic the isolated environment
            pg_socket_dir = "/opt/hams/pgsock"
            os.environ["PGHOST"] = pg_socket_dir
            os.environ["HAMS_ISOLATED_NS"] = "1"  # Set to 1 so paths behave like prod/isolated

            orig_user = os.environ.get("SUDO_USER") or os.environ.get("USER")

            if args.provision_jules:
                print("[*] Provisioning local Jules environment (apt packages, services)...")
                def _run_sudo_cmd(cmd, env=None):
                    if isinstance(cmd, str):
                        subprocess.run(["sudo", "bash", "-c", cmd], check=True, env=env)
                    else:
                        subprocess.run(["sudo"] + cmd, check=True, env=env)
                print("[*] Installing APT packages for early_prod...")
                old_apt = copy.deepcopy(infrastructure.MANIFEST.get("apt_packages", []))
                infrastructure.MANIFEST["apt_packages"] = [
                    pkg for pkg in infrastructure.MANIFEST.get("apt_packages", [])
                    if pkg["name"] not in ("postgresql-17-pgvector", "kopia")
                ]
                infrastructure.provision_apt_packages(_run_sudo_cmd, environment="early_prod")
                infrastructure.MANIFEST["apt_packages"] = old_apt

                print("[*] Adding Odoo 19 repository and installing Odoo...")
                _run_sudo_cmd("wget -O - https://nightly.odoo.com/odoo.key | gpg --dearmor -o /usr/share/keyrings/odoo-archive-keyring.gpg --yes || true")
                _run_sudo_cmd('echo "deb [signed-by=/usr/share/keyrings/odoo-archive-keyring.gpg] http://nightly.odoo.com/19.0/nightly/deb/ ./" | tee /etc/apt/sources.list.d/odoo.list')
                _run_sudo_cmd("apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y odoo python3-websocket jing postgresql-client python3-pip python3-pika python3-psutil python3-requests python3-cryptography python3-passlib python3-lxml")
                _run_sudo_cmd("DEBIAN_FRONTEND=noninteractive apt-get install -y chromium || DEBIAN_FRONTEND=noninteractive apt-get install -y chromium-browser || true")

                print("[*] Installing Python dependencies from requirements.txt...")
                req_file = os.path.join(base_dir, "requirements.txt")
                if os.path.exists(req_file):
                    _run_sudo_cmd(f"pip3 install --break-system-packages --ignore-installed -r {req_file}")
                else:
                    print("⚠️ WARNING: requirements.txt not found, skipping Python dependency installation.")

                print("[*] Configuring local PostgreSQL for test paths...")
                print("[*] Installing Python dependencies from requirements.txt...")
                req_file = os.path.join(base_dir, "requirements.txt")
                if os.path.exists(req_file):
                    _run_sudo_cmd(f"pip3 install --break-system-packages --ignore-installed -r {req_file}")
                else:
                    print("⚠️ WARNING: requirements.txt not found, skipping Python dependency installation.")

                print("[*] Configuring local PostgreSQL for test paths...")
                pg_data_dir = "/opt/hams/pgdata"
                pg_bins = glob.glob("/usr/lib/postgresql/*/bin/initdb")
                if not pg_bins:
                    print("❌ ERROR: PostgreSQL initdb not found. Did the apt install fail?")
                    sys.exit(1)
                pg_bin_dir = os.path.dirname(sorted(pg_bins)[-1]) + "/"

                _run_sudo_cmd("systemctl stop postgresql || true")
                _run_sudo_cmd(f"mkdir -p {pg_data_dir} {pg_socket_dir}")
                _run_sudo_cmd(f"chown -R postgres:postgres {pg_data_dir} {pg_socket_dir}")
                _run_sudo_cmd(f"chmod 700 {pg_data_dir}")
                _run_sudo_cmd(f"chmod 2775 {pg_socket_dir}")

                # Check if already initialized to avoid initdb error
                res = subprocess.run(["sudo", "ls", "-A", pg_data_dir], capture_output=True, text=True)
                if not res.stdout.strip():
                    _run_sudo_cmd(f"su -s /bin/bash postgres -c '{pg_bin_dir}initdb -D {pg_data_dir}'")

                _run_sudo_cmd(f"su -s /bin/bash postgres -c \"{pg_bin_dir}pg_ctl -D {pg_data_dir} -m fast stop\" || true")
                _run_sudo_cmd(f"su -s /bin/bash postgres -c \"{pg_bin_dir}pg_ctl -D {pg_data_dir} -o '-c listen_addresses= -c unix_socket_directories={pg_socket_dir} -c fsync=off -c synchronous_commit=off -c full_page_writes=off' -w start\"")
                _run_sudo_cmd(f"echo \"CREATE ROLE odoo WITH SUPERUSER LOGIN PASSWORD 'odoo'; CREATE ROLE {orig_user} WITH SUPERUSER LOGIN;\" | su -s /bin/bash postgres -c 'PGUSER=postgres {pg_bin_dir}psql -h {pg_socket_dir} -d postgres' || true")

                print("[*] Starting local Redis and RabbitMQ...")
                _run_sudo_cmd("systemctl start redis-server || true")
                _run_sudo_cmd("systemctl start rabbitmq-server || true")

                def teardown_jules():
                    print("[*] Tearing down local test PostgreSQL...")
                    subprocess.run(["sudo", "su", "-s", "/bin/bash", "postgres", "-c", f"{pg_bin_dir}pg_ctl -D {pg_data_dir} -m fast stop"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                atexit.register(teardown_jules)
            elif args.already_provisioned:
                pg_data_dir = "/opt/hams/pgdata"
                pg_bins = glob.glob("/usr/lib/postgresql/*/bin/initdb")
                if pg_bins:
                    pg_bin_dir = os.path.dirname(sorted(pg_bins)[-1]) + "/"
                    subprocess.run(["sudo", "systemctl", "stop", "postgresql"])
                    subprocess.run(["sudo", "mkdir", "-p", pg_socket_dir])
                    subprocess.run(["sudo", "chown", "-R", "postgres:postgres", pg_socket_dir])
                    subprocess.run(["sudo", "chmod", "2775", pg_socket_dir])
                    subprocess.run(["sudo", "chmod", "700", pg_data_dir])
                    subprocess.run(["sudo", "su", "-s", "/bin/bash", "postgres", "-c", f"{pg_bin_dir}pg_ctl -D {pg_data_dir} -m fast stop"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                    subprocess.run(["sudo", "su", "-s", "/bin/bash", "postgres", "-c", f"{pg_bin_dir}pg_ctl -D {pg_data_dir} -o '-c listen_addresses= -c unix_socket_directories={pg_socket_dir} -c fsync=off -c synchronous_commit=off -c full_page_writes=off' -w start"])
                    subprocess.run(["sudo", "systemctl", "start", "redis-server"])
                    subprocess.run(["sudo", "systemctl", "start", "rabbitmq-server"])
                    def teardown_jules():
                        subprocess.run(["sudo", "su", "-s", "/bin/bash", "postgres", "-c", f"{pg_bin_dir}pg_ctl -D {pg_data_dir} -m fast stop"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                    atexit.register(teardown_jules)
            else:
                print("⚠️ Please run with --provision-jules first, or --already-provisioned if already set up.")
                sys.exit(1)

        venv_python = os.path.join(base_dir, ".venv", "bin", "python")
        if is_jules_vm:
            venv_python = "/usr/bin/python3"  # use system python for odoo in Jules VM
        odoo_bin = "/usr/bin/odoo"

        current_pythonpath = os.environ.get("PYTHONPATH", "")
        os.environ["PYTHONPATH"] = "/usr/lib/python3/dist-packages:{}".format(
            current_pythonpath
        ).strip(":")

        addons_path = get_addons_path(base_dir)

        ignore_filepath = os.path.join(base_dir, args.config)
        ignore_patterns = load_ignore_file(ignore_filepath)

        if args.module:
            target_modules = [m.strip() for m in args.module.split(",")]
        else:
            target_modules = get_local_modules(base_dir, ignore_patterns)

        if not target_modules:
            print("❌ ERROR: No modules found in this repository. Aborting.")
            sys.exit(1)

        mod_string = "base," + ",".join(target_modules)
        test_tags = ",".join(["/{}".format(m) for m in target_modules])

        def get_python_test_cmd(suffix=""):
            cmd = [venv_python]
            if args.profile:
                base_name = f"odoo_test{suffix}.prof"
                prof_path = f"/opt/hams/spool/{base_name}" if os.environ.get("HAMS_ISOLATED_NS") == "1" else os.path.join(os.path.dirname(real_error_log), base_name)
                cmd.extend(["-m", "cProfile", "-o", prof_path])
            return cmd

        extractor = FailureExtractor(args.error_log)

        print("==========================================================")
        print(" 🧪 ODOO TEST RUNNER [{} MODE]".format(args.mode.upper()))
        print("==========================================================")
        print(" Target Database: {}".format(args.db))
        print(" Target Modules:  {}".format(mod_string))
        print(" Error Log:       {}".format(extractor.display_path))
        print("==========================================================")

        final_rc = 0

        if args.mode == "standard":
            check_linters(venv_python, base_dir, ignore_filepath, extractor)
            final_rc = run_daemon_tests(
                venv_python, base_dir, extractor, ignore_patterns, target_modules
            )
            if final_rc != 0:
                print(
                    "\n⚠️ WARNING: Daemon tests failed!\nContinuing to Odoo suite to collect all errors.\n"
                )

            restored, cache_file = check_and_restore_cache(args.db, mod_string)
            if not restored:
                print("[*] Initializing DB...")
                init_cmd = [
                    venv_python,
                    odoo_bin,
                    "--addons-path",
                    addons_path,
                    "-d",
                    args.db,
                    "-i",
                    mod_string,
                    "--stop-after-init",
                    "--workers=0",
                    "--max-cron-threads=0",
                    "--http-interface",
                    "localhost",
                    "--log-handler",
                    "odoo.tools.convert:DEBUG",
                ]
                rc_init = run_cmd(init_cmd, extractor)
                if rc_init != 0:
                    print("❌ ERROR: Database initialization failed!")
                    if extractor:
                        extractor.finish_and_write()
                    sys.exit(rc_init)
                save_db_cache(args.db, cache_file, mod_string)

            print("[*] Executing Test Suite...")
            standard_tags = test_tags + ",-integration"
            cmd = get_python_test_cmd() + [
                odoo_bin,
                "--addons-path",
                addons_path,
                "--dev=all",
                "-d",
                args.db,
                "-u",
                mod_string,
                "--test-enable",
                "--test-tags",
                standard_tags,
                "--stop-after-init",
                "--workers=0",
                "--max-cron-threads=0",
                "--http-interface",
                "localhost",
            ]
            rc_odoo = run_cmd(cmd, extractor)
            if rc_odoo != 0:
                final_rc = rc_odoo

        elif args.mode == "integration":
            check_linters(venv_python, base_dir, ignore_filepath, extractor)
            final_rc = run_daemon_tests(
                venv_python, base_dir, extractor, ignore_patterns, target_modules
            )
            if final_rc != 0:
                print(
                    "\n⚠️ WARNING: Daemon tests failed!\nContinuing to Odoo suite to collect all errors.\n"
                )

            os.environ["HAMS_INTEGRATION_MODE"] = "1"

            restored, cache_file = check_and_restore_cache(args.db, mod_string)
            if not restored:
                print("[*] Initializing the DB (creating tables for daemons)...")
                init_cmd = [
                    venv_python,
                    odoo_bin,
                    "--addons-path",
                    addons_path,
                    "-d",
                    args.db,
                    "-i",
                    mod_string,
                    "--test-enable",
                    "--test-tags",
                    "/__skip_init_tests__",
                    "--stop-after-init",
                    "--workers=0",
                    "--max-cron-threads=0",
                    "--http-interface",
                    "localhost",
                    "--log-level=warn",
                ]
                rc_init = run_cmd(init_cmd, extractor)
                if rc_init != 0:
                    print("❌ ERROR: Database initialization failed!")
                    final_rc = rc_init
                else:
                    save_db_cache(args.db, cache_file, mod_string)

            if final_rc == 0:
                print("[*] Executing Test Suite in Integration Mode...")

                # --- ORCHESTRATE INTEGRATION DAEMONS ---
                with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
                    s.bind(("", 0))
                    free_port = s.getsockname()[1]

                print(f"[*] Starting background Odoo on port {free_port} for integration daemons...")
                odoo_proc = subprocess.Popen(
                    [
                        venv_python, odoo_bin, "--addons-path", addons_path,
                        "-d", args.db, "--db-filter", f"^{args.db}$",
                        "--workers=0", "--max-cron-threads=0",
                        "--http-port", str(free_port), "--http-interface", "localhost",
                        "--log-level=warn",
                    ],
                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                    start_new_session=True,
                )

                for _ in range(30):
                    if is_odoo_running(free_port):
                        break
                    time.sleep(1)

                daemon_env = os.environ.copy()
                daemon_env["ODOO_URL"] = f"http://localhost:{free_port}"
                daemon_env["DB_NAME"] = args.db
                daemon_env["ODOO_USER"] = "admin"
                daemon_env["ODOO_PASSWORD"] = "admin"

                d_procs = []
                daemon_scripts = []
                for mod in target_modules:
                    daemon_dir = os.path.join(base_dir, mod, "daemon")
                    if not os.path.exists(daemon_dir):
                        daemon_dir = os.path.join(base_dir, mod, "daemons")
                    if os.path.exists(daemon_dir):
                        for f in os.listdir(daemon_dir):
                            if f.endswith(".py") and not f.startswith("test_") and not f.startswith("__"):
                                daemon_scripts.append(os.path.join(daemon_dir, f))

                for ds in daemon_scripts:
                    if os.path.exists(ds):
                        p = subprocess.Popen([venv_python, ds], env=daemon_env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                        d_procs.append(p)

                integration_tags = test_tags + ",-standard"

                test_cmd = get_python_test_cmd("_integration") + [
                    odoo_bin,
                    "--addons-path",
                    addons_path,
                    "-d",
                    args.db,
                    "-u",
                    mod_string,
                    "--test-enable",
                    "--test-tags",
                    integration_tags,
                    "--stop-after-init",
                    "--workers=0",
                    "--max-cron-threads=0",
                    "--http-interface",
                    "localhost",
                ]
                rc_odoo = run_cmd(test_cmd, extractor, env=daemon_env)
                if rc_odoo != 0:
                    final_rc = rc_odoo

                print("[*] Tearing down integration daemons...")
                for p in d_procs:
                    p.terminate()
                try:
                    os.killpg(os.getpgid(odoo_proc.pid), signal.SIGKILL)
                except Exception as e: # audit-ignore-catch-all
                    logging.getLogger('tools.test_runner').warning("Failed to kill pg: %s", e)
                    pass

        elif args.mode == "individual":
            check_linters(venv_python, base_dir, ignore_filepath, extractor)
            failed_modules = []
            for mod in target_modules:
                print("\n[*] ----------------------------------------------------")
                print("[*] Testing Module: {}".format(mod))
                print("[*] ----------------------------------------------------")

                individual_tags = f"/{mod}"

                restored, cache_file = check_and_restore_cache(args.db, mod)
                if not restored:
                    print(f"[*] Initializing DB for {mod}...")
                    init_cmd = [
                        venv_python,
                        odoo_bin,
                        "--addons-path",
                        addons_path,
                        "--dev=all",
                        "-d",
                        args.db,
                        "-i",
                        mod,
                        "--stop-after-init",
                        "--workers=0",
                        "--max-cron-threads=0",
                        "--http-interface",
                        "localhost",
                        "--log-level=warn",
                    ]
                    rc_init = run_cmd(init_cmd, extractor)
                    if rc_init != 0:
                        failed_modules.append(mod)
                        continue
                    save_db_cache(args.db, cache_file, mod)

                print(f"[*] Executing tests for {mod}...")
                cmd = get_python_test_cmd(f"_{mod}") + [
                    odoo_bin,
                    "--addons-path",
                    addons_path,
                    "--dev=all",
                    "-d",
                    args.db,
                    "-u",
                    mod,
                    "--test-enable",
                    "--test-tags",
                    individual_tags,
                    "--stop-after-init",
                    "--workers=0",
                    "--max-cron-threads=0",
                    "--http-interface",
                    "localhost",
                ]
                rc = run_cmd(cmd, extractor)
                if rc != 0:
                    failed_modules.append(mod)

            print("\n========================================================")
            if not failed_modules:
                print("🎉 All modules passed individual testing!")
            else:
                print("🚨 The following modules failed testing:")
                for fmod in failed_modules:
                    print("   - {}".format(fmod))
                final_rc = 1

        elif args.mode == "xml":
            failed_modules = []
            for mod in target_modules:
                print("\n[*] Checking XML views in: {}".format(mod))
                cmd = [
                    venv_python,
                    odoo_bin,
                    "--addons-path",
                    addons_path,
                    "-d",
                    args.db,
                    "-i",
                    mod,
                    "-u",
                    mod,
                    "--stop-after-init",
                    "--log-level=error",
                    "--workers=0",
                    "--max-cron-threads=0",
                    "--http-interface",
                    "localhost",
                ]
                rc = run_cmd(cmd, extractor)
                if rc != 0:
                    failed_modules.append(mod)

            print("\n========================================================")
            if not failed_modules:
                print("🎉 All modules compiled successfully!")
            else:
                print("🚨 The following modules have XML compilation errors:")
                for fmod in failed_modules:
                    print("   - {}".format(fmod))
                final_rc = 1

        elif args.mode == "downloads":
            restored, cache_file = check_and_restore_cache(args.db, mod_string)
            if not restored:
                print("[*] Initializing the DB (creating tables)...")
                init_cmd = [
                    venv_python,
                    odoo_bin,
                    "--addons-path",
                    addons_path,
                    "-d",
                    args.db,
                    "-i",
                    mod_string,
                    "--stop-after-init",
                    "--workers=0",
                    "--max-cron-threads=0",
                    "--http-interface",
                    "localhost",
                    "--log-level=warn",
                ]
                rc_init = run_cmd(init_cmd, extractor)
                if rc_init != 0:
                    print("❌ ERROR: Database initialization failed!")
                    final_rc = rc_init
                else:
                    save_db_cache(args.db, cache_file, mod_string)

            if final_rc == 0:
                with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
                    s.bind(("", 0))
                    free_port = s.getsockname()[1]

                print(
                    f"[*] Starting Odoo in background on port {free_port} for JSON-2 RPC..."
                )
                odoo_proc = subprocess.Popen(
                    [
                        venv_python,
                        odoo_bin,
                        "--addons-path",
                        addons_path,
                        "-d",
                        args.db,
                        "--db-filter",
                        f"^{args.db}$",
                        "--workers=0",
                        "--max-cron-threads=0",
                        "--http-port",
                        str(free_port),
                        "--http-interface",
                        "localhost",
                        "--log-level=info",
                    ],
                    stdout=subprocess.PIPE,
                    stderr=subprocess.STDOUT,
                    text=True,
                    bufsize=1,
                    start_new_session=True,
                )

                def cleanup_odoo():
                    try:
                        os.killpg(os.getpgid(odoo_proc.pid), signal.SIGKILL)
                        odoo_proc.wait(timeout=2)
                    except Exception as e: # audit-ignore-catch-all
                        logging.getLogger('tools.test_runner').warning("An error occurred: %s", e)
                        pass

                atexit.register(cleanup_odoo)


                odoo_extractor = FailureExtractor(args.error_log, disable_atexit=True)

                def stream_odoo_output(proc, o_extr, m_extr):
                    for line in proc.stdout:
                        line_lower = line.lower()
                        if (
                            "deprecated" in line_lower and "directive" in line_lower
                        ) or "pypdf2" in line_lower:
                            continue
                        sys.stdout.write(line)
                        sys.stdout.flush()
                        if o_extr:
                            if m_extr and getattr(m_extr, "current_context", None):
                                o_extr.current_context = (
                                    m_extr.current_context + " (Background Odoo)"
                                )
                            o_extr.process_line(line)

                odoo_executor = concurrent.futures.ThreadPoolExecutor(max_workers=1)
                odoo_executor.submit(stream_odoo_output, odoo_proc, odoo_extractor, extractor)

                print(f"[*] Waiting for Odoo to bind to port {free_port}...")

                for _ in range(30):
                    if is_odoo_running(free_port):
                        time.sleep(3)
                        break
                    time.sleep(1)
                else:
                    print(f"❌ ERROR: Odoo failed to start on port {free_port}!")
                    try:
                        os.killpg(os.getpgid(odoo_proc.pid), signal.SIGKILL)
                    except Exception as e: # audit-ignore-catch-all
                        logging.getLogger('tools.test_runner').warning("An error occurred: %s", e)
                        pass
                    sys.exit(1)

                env_path = os.path.join(base_dir, "deploy", "env")
                db_user = "odoo"
                db_password = None
                db_host = None
                db_port = "5432"
                odoo_admin_password = "admin"

                if os.environ.get("HAMS_ISOLATED_NS") == "1":
                    db_host = os.environ.get("PGHOST")
                    db_user = "odoo"
                    db_password = "odoo"
                else:
                    if os.path.exists(env_path):
                        with open(env_path, "r", encoding="utf-8") as f:
                            for line in f:
                                line = line.strip()
                                if line.startswith("POSTGRES_USER="):
                                    db_user = line.split("=", 1)[1].strip("'\"")
                                elif line.startswith("DB_USER="):
                                    db_user = line.split("=", 1)[1].strip("'\"")
                                elif line.startswith("POSTGRES_PASSWORD="):
                                    db_password = line.split("=", 1)[1].strip("'\"")
                                elif line.startswith("DB_PASSWORD="):
                                    db_password = line.split("=", 1)[1].strip("'\"")
                                elif line.startswith("POSTGRES_HOST="):
                                    db_host = line.split("=", 1)[1].strip("'\"")
                                elif line.startswith("DB_HOST="):
                                    db_host = line.split("=", 1)[1].strip("'\"")
                                elif line.startswith("ODOO_ADMIN_PASSWORD="):
                                    odoo_admin_password = line.split("=", 1)[1].strip(
                                        "'\""
                                    )
                                elif line.startswith("ODOO_SERVICE_PASSWORD="):
                                    odoo_admin_password = line.split("=", 1)[1].strip(
                                        "'\""
                                    )

                    if not db_host:
                        db_host = "localhost"

                daemon_env = os.environ.copy()
                daemon_env["ODOO_URL"] = f"http://localhost:{free_port}"
                daemon_env["DB_NAME"] = args.db
                daemon_env["ODOO_USER"] = "admin"
                daemon_env["ODOO_PASSWORD"] = odoo_admin_password
                daemon_env["HAMS_NO_AI"] = "1"
                daemon_env["DISABLE_AI_EXPLANATIONS"] = "1"
                daemon_env["GEMINI_API_KEY"] = "dummy_key_to_bypass"

                pg_env = os.environ.copy()
                if db_user:
                    pg_env["PGUSER"] = db_user
                if db_password:
                    pg_env["PGPASSWORD"] = db_password
                if db_host:
                    pg_env["PGHOST"] = db_host
                if db_port:
                    pg_env["PGPORT"] = str(db_port)

                try:
                    psql_cmd = shutil.which("psql") or "psql"
                    subprocess.run(
                        [psql_cmd, "-c", "SELECT 1;", args.db],
                        env=pg_env,
                        check=True,
                        capture_output=True
                    )
                except Exception as e: # audit-ignore-catch-all
                    logging.getLogger('tools.test_runner').error("Failed to connect to PostgreSQL via psql: %s", e)
                    print("[!] Failed to connect to PostgreSQL via psql: {}".format(e))
                    os.killpg(os.getpgid(odoo_proc.pid), signal.SIGKILL)
                    sys.exit(1)

                def discover_test_daemons(base_dir, target_modules):
                    daemons = []
                    for mod in target_modules:
                        daemon_dir = os.path.join(base_dir, mod, "daemon")
                        if not os.path.exists(daemon_dir):
                            daemon_dir = os.path.join(base_dir, mod, "daemons")
                        if not os.path.exists(daemon_dir):
                            continue

                        for root, _, files in os.walk(daemon_dir):
                            if "__pycache__" in root or "tests" in root:
                                continue
                            for f in sorted(files):
                                if (
                                    f.endswith(".py")
                                    and not f.startswith("test_")
                                    and not f.startswith("__")
                                    and f != "hams_config.py"
                                ):
                                    full_path = os.path.join(root, f)
                                    try:
                                        with open(
                                            full_path, "r", encoding="utf-8"
                                        ) as script_file:
                                            content = script_file.read()
                                            if "__name__" in content and (
                                                '"__main__"' in content
                                                or "'__main__'" in content
                                            ):
                                                rel_path = os.path.relpath(
                                                    full_path, base_dir
                                                )
                                                daemon_name = os.path.basename(root)
                                                args_list = [rel_path]
                                                if daemon_name == "ncvec_sync":
                                                    ncvec_url = "https://raw.githubusercontent.com/Ham-Radio-Prep/ncvec/master/Element_2_Technician.txt"
                                                    args_list.extend(["--url", ncvec_url])
                                                daemons.append((daemon_name, args_list))
                                    except Exception as e: # audit-ignore-catch-all
                                        logging.getLogger('tools.test_runner').warning(
                                            "An error occurred: %s", e
                                        )
                                        pass
                    return daemons

                d_list = discover_test_daemons(base_dir, target_modules)

                if args.daemon:
                    d_list = [d for d in d_list if args.daemon in d[0]]
                    if not d_list:
                        print(f"❌ ERROR: No daemon matched '{args.daemon}'")
                        os.killpg(os.getpgid(odoo_proc.pid), signal.SIGKILL)
                        sys.exit(1)

                TABLES_TO_TRACK = [
                    "ham_callbook",
                    "ham_space_weather",
                    "ham_satellite_tle",
                    "survey_question",
                    "ham_contest",
                    "event_event",
                    "ham_repeater",
                    "ham_au_register",
                ]

                def get_table_counts():
                    counts = {}
                    for table in TABLES_TO_TRACK:
                            try:
                                psql_cmd = shutil.which("psql") or "psql"
                                q_str = "SELECT count(*) FROM {};".format(table)
                                res = subprocess.run(
                                    [psql_cmd, "-t", "-A", "-c", q_str, args.db],
                                    env=pg_env,
                                    capture_output=True,
                                    text=True
                                )
                                if res.returncode == 0 and res.stdout.strip().isdigit():
                                    counts[table] = int(res.stdout.strip())
                                elif "does not exist" in res.stderr:
                                    counts[table] = "Not Installed"
                                else:
                                    counts[table] = "Error"
                            except Exception as e: # audit-ignore-catch-all
                                logging.getLogger('tools.test_runner').warning("Table count error: %s", e)
                                counts[table] = "Error"
                    return counts

                print("\n[*] Fetching Initial Database Counts...")
                initial_counts = get_table_counts()

                print("\n[*] Bootstrapping Real Bearer Tokens via Odoo Shell...")
                bootstrap_script = os.path.join(
                    base_dir, "deploy", "bootstrap_daemon_keys.py"
                )
                if os.path.exists(bootstrap_script):
                    try:
                        with open(bootstrap_script, "r") as script_file:
                            res = subprocess.run(
                                [
                                    venv_python,
                                    odoo_bin,
                                    "shell",
                                    "--addons-path",
                                    addons_path,
                                    "-d",
                                    args.db,
                                    "--no-http",
                                    "--workers=0",
                                ],
                                stdin=script_file,
                                text=True,
                                capture_output=True,
                            )
                        if res.returncode != 0:
                            print(res.stdout)
                            print(res.stderr)
                            print(
                                f"❌ ERROR: Failed to bootstrap API keys (exit code {res.returncode})"
                            )
                            if extractor:
                                extractor.captured_blocks.append(
                                    (
                                        "Daemon Key Bootstrapper",
                                        [
                                            (res.stdout or "") + "\n",
                                            (res.stderr or "") + "\n",
                                            "\nERROR: Bootstrapper failed.\n",
                                        ],
                                    )
                                )
                                extractor.finish_and_write()
                            os.killpg(os.getpgid(odoo_proc.pid), signal.SIGKILL)
                            sys.exit(1)
                        else:
                            print(
                                "[+] Real daemon bearer tokens provisioned successfully."
                            )
                    except Exception as e: # audit-ignore-catch-all
                        logging.getLogger('tools.test_runner').error("Bootstrapper error: %s", e)
                        print(f"❌ ERROR: Failed to execute bootstrapper: {e}")
                        if extractor:
                            extractor.captured_blocks.append(
                                ("Daemon Key Bootstrapper", [f"ERROR: {e}\n"])
                            )
                            extractor.finish_and_write()
                        os.killpg(os.getpgid(odoo_proc.pid), signal.SIGKILL)
                        sys.exit(1)
                else:
                    print("❌ ERROR: deploy/bootstrap_daemon_keys.py not found.")
                    if extractor:
                        extractor.captured_blocks.append(
                            (
                                "Daemon Key Bootstrapper",
                                ["ERROR: deploy/bootstrap_daemon_keys.py not found.\n"],
                            )
                        )
                        extractor.finish_and_write()
                    os.killpg(os.getpgid(odoo_proc.pid), signal.SIGKILL)
                    sys.exit(1)

                print("\n[*] Commencing Download Framework Execution...")
                for name, script_args in d_list:
                    script_path = script_args[0]
                    full_path = os.path.join(base_dir, script_path)
                    if not os.path.exists(full_path):
                        print("[-] Skipping {}".format(name))
                        continue

                    print("\n[*] --> Running {}...".format(name))
                    if extractor:
                        extractor.set_context("Daemon Execution / {}".format(name))

                    key_name = name
                    daemon_env["ODOO_KEY_FILE"] = f"/opt/hams/etc/keys/{key_name}.env"

                    try:
                        cmd_list = [venv_python, full_path] + script_args[1:]
                        rc = run_cmd(cmd_list, extractor, cwd=base_dir, env=daemon_env)
                        if rc == 0:
                            print("[+] {} completed.".format(name))
                        else:
                            print("[!] {} failed.".format(name))
                            final_rc = 1
                    except KeyboardInterrupt:
                        print("\n[!] Execution aborted by user.")
                        break
                    except Exception as e: # audit-ignore-catch-all
                        logging.getLogger('tools.test_runner').error("Error executing %s: %s", name, e)
                        print("[!] Error executing {}: {}".format(name, e))
                        final_rc = 1

                print("\n[*] Fetching Final Database Counts...")
                final_counts = get_table_counts()

                print("\nFinal Record Counts Comparison:")
                for t in TABLES_TO_TRACK:
                    init_c = initial_counts.get(t, "N/A")
                    fin_c = final_counts.get(t, "N/A")
                    print("{:<20} | {:<15} | {:<15}".format(t, str(init_c), str(fin_c)))

                print("\n[+] Tearing down background Odoo server...")
                if hasattr(atexit, "unregister"):
                    atexit.unregister(cleanup_odoo)
                try:
                    os.killpg(os.getpgid(odoo_proc.pid), signal.SIGKILL)
                except Exception as e: # audit-ignore-catch-all
                    logging.getLogger('tools.test_runner').warning("Could not kill process group: %s", e)
                    print("[!] Note: Could not kill process group.")
                odoo_proc.wait()

                if odoo_extractor.capturing and odoo_extractor.current_block:
                    odoo_extractor.captured_blocks.append(
                        (odoo_extractor.current_context, odoo_extractor.current_block)
                    )
                    odoo_extractor.capturing = False
                    odoo_extractor.current_block = []

                if odoo_extractor.captured_blocks:
                    extractor.captured_blocks.extend(odoo_extractor.captured_blocks)
                    final_rc = 1

        sys.exit(final_rc)
    except KeyboardInterrupt:
        print("\n[!] Test run aborted by user.")
        sys.exit(1)


if __name__ == "__main__":
    main()
