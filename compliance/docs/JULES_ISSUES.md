# Jules Issues & Observations

## Automated Verification Environment
- **Chrome Startup Issue:** Encountered `Failed to connect to the bus: Address does not contain a colon` errors during UI tours. Resolved by installing `dbus-x11` in the VM environment.
- **Permission Denied (PostgreSQL):** Encountered permission issues when Odoo attempted to connect to the PostgreSQL socket at `/opt/hams/pgsock`. Resolved by ensuring proper directory permissions.
- **Permission Denied (/home/jules/.local):** Encountered permission issues when Odoo attempted to create the session directory. Resolved by providing an explicit `--data-dir` to the Odoo server.

## Module Observations
- **Cookie Bar Default:** The module now ensures `cookies_bar` is enabled by default for new websites via model inheritance in `compliance/models/compliance_config.py`.
- **Footer Links:** Added automated footer link injection to ensure legal pages are globally accessible.
- **Template DRY:** Refactored repetitive legal links into a shared sub-template.
