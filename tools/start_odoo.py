#!/usr/bin/env python3
import os
import sys
import socket
import shutil
import subprocess


def check_path(filepath, description):
    if not os.path.exists(filepath):
        print(f"[FAIL] Missing {description}: {filepath}")
        return False
    print(f"[OK] Found {description}.")
    return True


def check_port(host, port, service_name):
    try:
        with socket.create_connection((host, port), timeout=3):
            print(f"[OK] {service_name} is active on port {port}.")
            return True
    except OSError:
        print(f"[FAIL] {service_name} is unreachable on port {port}.")
        return False


def check_daemon(process_name, description):
    try:
        # grep -v grep prevents matching the pgrep command itself
        cmd = ["pgrep", "-f", process_name]
        subprocess.check_output(cmd, stderr=subprocess.DEVNULL)
        print(f"[OK] Daemon running: {description}")
        return True
    except subprocess.CalledProcessError:
        print(f"[FAIL] Daemon offline: {description}")
        return False


def main():
    print("=== Hams.com Odoo Pre-Flight Check ===")
    success = True

    # 1. Check Executable & Environment
    cwd = os.getcwd()

    odoo_exec = shutil.which("odoo-bin") or shutil.which("odoo")
    if not odoo_exec:
        print("[FAIL] Odoo executable not found in PATH.")
        success = False
    else:
        print(f"[OK] Odoo executable found: {odoo_exec}")

    # 2. Check Configuration Files
    env_path = os.path.join(cwd, "deploy", ".env")
    conf_path = os.path.join(cwd, "deploy", "odoo.conf")

    success &= check_path(env_path, "Vault (.env)")
    success &= check_path(conf_path, "Odoo Config (odoo.conf)")

    # 3. Check Infrastructure Ports
    success &= check_port("localhost", 80, "Nginx (HTTP Edge)")
    success &= check_port("localhost", 5432, "PostgreSQL Database")
    success &= check_port("localhost", 6379, "Redis Cache")
    success &= check_port("localhost", 5672, "RabbitMQ Bus")

    # 4. Check Continuous Background Daemons (Process Check)
    continuous_daemons = [
        ("adif_processor.py", "ADIF Processor"),
        ("cache_manager", "Distributed Cache Manager"),
        ("dx_firehose.py", "DX Firehose"),
        ("dx_daemon.py", "Ham DX Daemon"),
        ("hams_local_relay.py", "Hams Local Relay"),
        ("pdns_sync.py", "PowerDNS Sync"),
        ("qrz_scraper.py", "QRZ Scraper"),
        ("generalized_monitor.py", "Pager Duty Monitor"),
    ]

    print("\n--- Checking Continuous Daemons ---")
    for process_string, desc in continuous_daemons:
        success &= check_daemon(process_string, desc)

    # 5. Check Periodic Daemons (Executable File Check)
    periodic_daemons = [
        ("daemons/amsat_tle_sync/amsat_sync.py", "AMSAT TLE Sync Script"),
        ("daemons/au_acma_sync/au_sync.py", "AU ACMA Sync Script"),
        ("daemons/br_anatel_sync/br_sync.py", "BR Anatel Sync Script"),
        ("daemons/de_bnetza_sync/de_sync.py", "DE BNetzA Sync Script"),
        ("daemons/event_sync/wa7bnm_contest_sync.py", "Event Sync Script"),
        ("daemons/fcc_uls_sync/fcc_sync.py", "FCC ULS Sync Script"),
        ("daemons/ised_canada_sync/ised_sync.py", "ISED Canada Sync Script"),
        ("daemons/lotw_eqsl_sync/lotw_eqsl_sync.py", "LoTW/eQSL Sync Script"),
        ("daemons/ncvec_sync/ncvec_sync.py", "NCVEC Sync Script"),
        ("daemons/noaa_swpc_sync/noaa_swpc_sync.py", "NOAA SWPC Sync Script"),
        ("daemons/nz_rsm_sync/nz_sync.py", "NZ RSM Sync Script"),
        ("daemons/uk_ofcom_sync/uk_sync.py", "UK Ofcom Sync Script"),
    ]

    print("\n--- Checking Periodic Daemon Executables ---")
    for filepath, desc in periodic_daemons:
        full_path = os.path.join(cwd, filepath)
        success &= check_path(full_path, desc)

    if not success:
        if os.environ.get("HAMS_SKIP_PREFLIGHT") == "1":
            print("\n[!] WARNING: Pre-flight failed, but HAMS_SKIP_PREFLIGHT=1. Proceeding anyway.")
        else:
            print("\n[!] CRITICAL: Pre-flight failed. Aborting startup.")
            print("    (Set HAMS_SKIP_PREFLIGHT=1 to bypass this check in development environments like Jules)")
            sys.exit(1)

    print("\n[+] All systems nominal. Starting Odoo...")

    # Silence Odoo's core framework noise (Cybercrud Policy)
    os.environ["PYTHONWARNINGS"] = "ignore::DeprecationWarning"

    # Safely replace the current process with the Odoo executable
    args = [odoo_exec] + sys.argv[1:]
    os.execv(odoo_exec, args)


if __name__ == "__main__":
    main()
