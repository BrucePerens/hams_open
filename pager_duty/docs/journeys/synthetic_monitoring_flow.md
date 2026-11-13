# Journey: Synthetic Monitoring Flow

This journey describes the execution of complex, multi-step browser-like journeys to verify end-to-end functionality.

## 1. Journey Definition
- **Scripting:** Journey scripts (e.g., Playwright or Bash) are defined as part of a `pager.check`.
- **Environment:** The `pager_synthetic_spooler.py` prepares the execution environment (`execute_check` in that file, described below).

## 2. Dynamic Dependency Resolution
- **Check:** The spooler checks for required binaries (e.g., `cloudflared`).
- **Healing:** If missing, it dynamically downloads the required static binaries to `/var/lib/odoo/hams_bin/`.

## 3. Execution & Spooling
- **Trigger:** `pager-synthetic-spooler.service` (`Type=simple`, `Restart=always`) runs `pager_synthetic_spooler.py`, which loops forever and runs each check when its own `interval` has elapsed; there is no timer unit.
- **Isolated Run:** The script executes in a subprocess.
- **Capture:** Output and error codes are captured.

## 4. Reporting
- **Success:** If the exit code is 0, the check is marked healthy.
- **Failure:** If the script fails, an error message is generated (e.g., "Execution timed out").
- **Alerting:** The failure is reported back to Odoo, triggering the standard incident lifecycle if necessary.

## 5. Sibling Spoolers: Hardware Health and One-Shot Journeys

- **SMART Disk Health:** A separate, simpler spooler, `pager_smart_spooler.py`, walks the host's own block devices and writes each one's real SMART health data (temperature, reallocated-sector count, overall pass/fail) into a spool file `generate_smart_spool()` [@ANCHOR: generate_smart_spool] produces -- the same "write a JSON file the monitor daemon reads back" pattern as the synthetic journey spooler, just for hardware rather than scripted journeys.

- **Synthetic Spooler Entrypoint:** The synthetic journey spooler's own `main()` [@ANCHOR: synthetic_spooler_main] is what ties steps 1 through 4 together on a real loop pass -- reading the check's config, preparing the environment, running the scripted journey, and writing the resulting spool file in one pass, so `pager-synthetic-spooler.service` only ever has one entrypoint to invoke.

## Function reference

### `execute_check(check)` of `pager_synthetic_spooler.py` [@ANCHOR: synthetic_i18n]

Runs one `playwright`, `bash` or `executable` check and returns `(name, result)`. Argument: `check` (`dict` from the configuration: `name`, `type`, `interval`, `code_payload`, `executable_path`, `executable_args`, `sandbox_downloads`, `sandbox_network_access`). The result is `{"success": bool, "error": str}` plus `"output"` (the script's stdout) for `bash` on success. All work happens in a fresh temporary directory. First, each `sandbox_downloads` line `url|sha256|filename` (bash and executable checks) is fetched: the scheme must be http or https, the host must pass the SSRF guard (before the connection and again on the final URL), the connection is pinned to the validated address, the body is streamed with a 200 MB cap, its SHA-256 must equal the given checksum (else `Checksum mismatch`), and the file is made mode 0755. The `User-Agent` honestly names `Pager-Synthetic-Spooler/1.0` with a contact URL. Then the script or executable runs under `bwrap` (read-only root, private `/tmp`, user, IPC, PID, UTS and cgroup namespaces, no network unless `sandbox_network_access` is not `loopback`), working in `/tmp/workspace`, with `shell=False`, killed after `interval` seconds. A non-zero exit gives the stderr as the error; a timeout gives "Execution timed out". Every exception becomes `result["error"]`, never an escaped exception, so one bad check cannot crash the spooler loop. The headless i18n audit and download-limit tests cite this anchor: `test_00_i18n_headless_audit`, `test_02d_sandbox_downloads_aborts_on_a_response_exceeding_the_size_cap` and `test_02e_sandbox_downloads_identify_honestly_not_as_a_browser` in `tests/test_synthetic_spooler.py`.
