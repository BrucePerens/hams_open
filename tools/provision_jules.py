#!/usr/bin/env python3
"""
Standalone Jules VM Provisioning Script
Extracted from test.py to simplify testing architecture.
Must be run as root.
"""
import os
import sys
import subprocess
import pwd
import logging
import shutil
import glob
import functools

# Prevent Docker/VM stdout buffering from ruining chronological log integrity
print = functools.partial(print, flush=True)

# Ensure base_dir is in path so we can import infrastructure when running globally
sys.path.insert(0, "/app")
import infrastructure # noqa: E402

_logger = logging.getLogger(__name__)

def get_pg_bin(name):
    paths = glob.glob(f"/usr/lib/postgresql/*/bin/{name}")
    if paths:
        return sorted(paths)[-1]
    res = shutil.which(name)
    if not res:
        for p in [f"/usr/bin/{name}", f"/usr/local/bin/{name}"]:
            if os.path.exists(p): return p
        raise FileNotFoundError(f"Could not find PostgreSQL binary: {name}")
    return res

def provision_jules():
    os.chdir("/app")
    base_dir = "/app"

    if os.geteuid() != 0:
        print("[*] Elevating privileges (sudo) to provision Jules environment...")
        os.execvp("sudo", ["sudo", "-H", "-E", sys.executable, os.path.abspath(__file__)])

    orig_user = os.environ.get("SUDO_USER") or os.environ.get("USER")
    env_vars = dict(os.environ)
    env_vars["DEBIAN_FRONTEND"] = "noninteractive"

    def run_sys(cmd, **kw):
        print(f"[*] Running: {' '.join(cmd)}")
        if "env" not in kw:
            kw["env"] = env_vars
        return subprocess.run(cmd, check=True, **kw)

    print("[*] Provisioning APT Sources and Packages...")

    try:
        with open("/etc/hosts", "r") as f:
            hosts_content = f.read()
        if "redis" not in hosts_content:
            print("[*] Ensuring docker-compose hostnames resolve locally in /etc/hosts...")
            with open("/etc/hosts", "a") as f:
                f.write("\n127.0.0.1 redis rabbitmq postgres pdns memcached\n")
    except OSError as e:
        print(f"[*] WARNING: Failed to update /etc/hosts: {e}")

    is_hams_community = os.path.exists(os.path.join(base_dir, "zero_sudo", "__manifest__.py"))

    if not is_hams_community:
        target_clone = "/hams_community"
        if not os.path.exists(target_clone):
            print(f"[*] Sibling repository not found. Cloning hams_community to {target_clone}...")
            try:
                clone_env = dict(env_vars)
                clone_env["GIT_TERMINAL_PROMPT"] = "0"
                run_sys(["git", "clone", "https://github.com/BrucePerens/hams_community", target_clone], env=clone_env)
                if orig_user:
                    u_info = pwd.getpwnam(orig_user)
                    run_sys(["chown", "-R", f"{u_info.pw_uid}:{u_info.pw_gid}", target_clone])
            except subprocess.CalledProcessError as e:
                print(f"[*] WARNING: Failed to clone to {target_clone}: {e}")
                print("[!] DIAGNOSTIC FOR AI: The hams_community repository could not be cloned due to GitHub authentication restrictions in this headless VM.")
                print("    If the required 'zero_sudo' module is not present in your current working directory, the tests will fatally crash.")
                print("    ACTION: Document this blocker in JULES_ISSUES.md and gracefully conclude your session.")

    try:
        apt_opts = ["-o", "Dpkg::Options::=--force-confdef", "-o", "Dpkg::Options::=--force-confold", "-o", "Dpkg::Lock::Timeout=120"]

        # 1. Install prerequisites for adding repositories (Jules already provides curl natively)
        run_sys(["apt-get", "update"] + apt_opts)
        run_sys(["apt-get", "install", "-y"] + apt_opts + ["gnupg", "lsb-release"])
        # 3. Add Postgres repository
        run_sys(["bash", "-c", "curl -fsSL https://www.postgresql.org/media/keys/ACCC4CF8.asc | gpg --dearmor --yes -o /usr/share/keyrings/postgresql-keyring.gpg"])
        run_sys(["bash", "-c", "echo \"deb [signed-by=/usr/share/keyrings/postgresql-keyring.gpg] http://apt.postgresql.org/pub/repos/apt/ $(lsb_release -cs)-pgdg main\" > /etc/apt/sources.list.d/pgdg.list"])

        # 4. Add Odoo repository (via static files)
        infrastructure.provision_static_files(run_sys, env_vars, environment="prod")
        # 5. Final update with all repositories loaded
        run_sys(["apt-get", "update"] + apt_opts + ["--allow-insecure-repositories"])

        # 6. Collect all required packages (excluding those already in Jules: curl, python3-pip, build-essential)
        all_packages = [
            "python3-setuptools", "python3-stdeb", "dh-python", "python3-all",
            "fakeroot", "postgresql-common", "postgresql-client",
            "postgresql", "odoo"
        ]

        # Tools explicitly pre-installed by the Jules VM
        jules_provided = {"curl", "python3-pip", "build-essential"}

        for pkg_spec in infrastructure.MANIFEST.get("apt_packages", []):
            if "early_prod" in pkg_spec["environments"]:
                if pkg_spec["name"] not in jules_provided:
                    all_packages.append(pkg_spec["name"])

        # 6. Dynamically determine PostgreSQL major version for pgvector
        pg_res = subprocess.run(
            ["bash", "-c", "apt-cache depends postgresql | grep -Eo 'postgresql-[0-9]+' | head -n1 | grep -Eo '[0-9]+'"],
            capture_output=True, text=True
        )
        if pg_res.returncode == 0 and pg_res.stdout.strip():
            pg_major = pg_res.stdout.strip()
            all_packages.append(f"postgresql-{pg_major}-pgvector")

        # 7. Deduplicate and install all packages in a single transaction
        all_packages = sorted(list(set(all_packages)))
        run_sys(["apt-get", "install", "-y"] + apt_opts + all_packages)

        req_file = os.path.join(base_dir, "requirements.txt")
        if os.path.exists(req_file):
            try:
                pip_env = dict(env_vars)
                pip_env["PIP_ROOT_USER_ACTION"] = "ignore"
                # Using the system python3 to install to system packages
                run_sys(["/usr/bin/python3", "-m", "pip", "install", "--break-system-packages", "--ignore-installed", "-r", req_file], env=pip_env)
            except subprocess.CalledProcessError as e:
                print(f"[*] WARNING: pip install encountered an error: {e}. Continuing to ensure database provisioning completes.")

        print("[*] Preparing testing directories with production paths...")
        try:
            if orig_user:
                try:
                    u_info = pwd.getpwnam(orig_user)
                    user_tmp = os.path.join(u_info.pw_dir, "tmp")
                    os.makedirs(user_tmp, exist_ok=True)
                    os.chown(user_tmp, u_info.pw_uid, u_info.pw_gid)
                except KeyError as e:
                    _logger.debug("Original user %s not found: %s", orig_user, e)

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
                    os.chmod(d, 0o750)
                except OSError as e:
                    _logger.debug("Ignored OSError: %s", e)
        except KeyError as e:
            print(f"[*] WARNING: 'odoo' user not found during directory preparation: {e}")

    except subprocess.CalledProcessError as e:
        print(f"❌ ERROR: Failed to provision system packages: {e}")
        sys.exit(1)

if __name__ == "__main__":
    provision_jules()
