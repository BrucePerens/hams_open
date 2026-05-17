# -*- coding: utf-8 -*-
import subprocess
import json
import os
import sys
import logging

logger = logging.getLogger(__name__)

def generate_smart_spool():
    try:
        # 1. Autodiscover all block devices supporting SMART
        scan_res = subprocess.run(
            ["smartctl", "--scan", "-j"], capture_output=True, text=True, check=True
        )
        devices = json.loads(scan_res.stdout).get("devices", [])

        out = {}
        for dev in devices:
            name = dev.get("name")
            if name:
                # 2. Query overall health status (-H) for each device
                health_res = subprocess.run(
                    ["smartctl", "-H", "-j", name], capture_output=True, text=True
                )
                try:
                    out[name] = json.loads(health_res.stdout)
                except json.JSONDecodeError as e:
                    logger.warning("JSON decode error for SMART data: %s", e)

        # 3. Write to the read-only spool directory accessible by the main daemon
        spool_file = "/var/log/pager_smart_spool.json"

        # Atomic Write: Write to a tmp file and rename to prevent the main daemon from reading a partial write
        tmp_file = spool_file + ".tmp"
        with open(tmp_file, "w") as f:
            json.dump(out, f)

        os.chmod(tmp_file, 0o644)
        os.rename(tmp_file, spool_file)

    except Exception as e:
        logger.error(f"Failed to generate SMART spool: {e}")
        sys.exit(1)


if __name__ == "__main__":
    generate_smart_spool()
