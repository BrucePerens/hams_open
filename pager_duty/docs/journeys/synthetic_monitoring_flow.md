# Journey: Synthetic Monitoring Flow

This journey describes the execution of complex, multi-step browser-like journeys to verify end-to-end functionality.

## 1. Journey Definition
- **Scripting:** Journey scripts (e.g., Playwright or Bash) are defined as part of a `pager.check`.
- **Environment:** The `pager_synthetic_spooler.py` prepares the execution environment [@ANCHOR: synthetic_i18n].

## 2. Dynamic Dependency Resolution
- **Check:** The spooler checks for required binaries (e.g., `cloudflared`).
- **Healing:** If missing, it dynamically downloads the required static binaries to `/var/lib/odoo/hams_bin/`.

## 3. Execution & Spooling
- **Trigger:** The `pager-synthetic-spooler.timer` triggers the service.
- **Isolated Run:** The script executes in a subprocess.
- **Capture:** Output and error codes are captured.

## 4. Reporting
- **Success:** If the exit code is 0, the check is marked healthy.
- **Failure:** If the script fails, an error message is generated (e.g., "Execution timed out").
- **Alerting:** The failure is reported back to Odoo, triggering the standard incident lifecycle if necessary.
