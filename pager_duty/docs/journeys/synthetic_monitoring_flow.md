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

## 5. Sibling Spoolers: Hardware Health and One-Shot Journeys

- **SMART Disk Health:** A separate, simpler spooler, `pager_smart_spooler.py`, walks the host's own block devices and writes each one's real SMART health data (temperature, reallocated-sector count, overall pass/fail) into a spool file `generate_smart_spool()` [@ANCHOR: generate_smart_spool] produces -- the same "write a JSON file the monitor daemon reads back" pattern as the synthetic journey spooler, just for hardware rather than scripted journeys.

- **Synthetic Spooler Entrypoint:** The synthetic journey spooler's own `main()` [@ANCHOR: synthetic_spooler_main] is what ties steps 1 through 4 together on a real timer tick -- reading the check's config, preparing the environment, running the scripted journey, and writing the resulting spool file in one pass, so `pager-synthetic-spooler.timer` only ever has one entrypoint to invoke.
