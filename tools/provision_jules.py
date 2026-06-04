#!/usr/bin/env python3
"""
Standalone Jules VM Provisioning Script
Extracted from test.py to simplify testing architecture.
Must be run as root.
"""
import os
import sys
import subprocess
import logging

# Ensure base_dir is in path so we can import infrastructure when running globally
sys.path.insert(0, "/app")
import infrastructure # noqa: E402

logging.basicConfig(level=logging.INFO)
_logger = logging.getLogger(__name__)

def provision_jules():
    os.chdir("/app")
    base_dir = "/app"

    if os.geteuid() != 0:
        _logger.info("[*] Elevating privileges (sudo) to provision Jules environment...")
        os.execvp("sudo", ["sudo", "-H", "-E", sys.executable, os.path.abspath(__file__)])

    orig_user = os.environ.get("SUDO_USER") or os.environ.get("USER")
    env_vars = dict(os.environ)
    env_vars["DEBIAN_FRONTEND"] = "noninteractive"

    def run_sys(cmd, **kw):
        _logger.info(f"[*] Running: {' '.join(cmd)}")
        if "env" not in kw:
            kw["env"] = env_vars
        return subprocess.run(cmd, check=True, **kw)

    infrastructure.provision_jules_environment(run_sys, env_vars, base_dir, orig_user)

if __name__ == "__main__":
    provision_jules()
