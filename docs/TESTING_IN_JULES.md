# Testing in the Jules VM Environment

When working within the Jules VM on Ubuntu 24.04, the standard `test_runner.py` Linux Mount Namespace isolations (`unshare`) are restricted due to permission limitations of the environment.

Instead, the test runner detects the Jules VM automatically via the `IN_JULES_VM` or `JULES_SESSION_ID` environment variables. When detected, the runner bypasses namespace isolations and executes directly.

To facilitate this, new flags have been added to provision the dependencies required for testing natively on the VM.

## Prerequisites

Before running tests, ensure that a `tmp` directory exists in your home folder for log output:

```bash
mkdir -p ~/tmp
```

## 1. Initial Provisioning (First Run)

To bootstrap the local Ubuntu environment with Odoo 19, PostgreSQL, Redis, RabbitMQ, and the required Python dependencies, run the test runner with the `--provision-jules` flag:

```bash
IN_JULES_VM=1 python3 tools/test_runner.py --provision-jules
```

This command will:
- Add the Odoo 19 Nightly APT repository.
- Install `odoo` and other required system dependencies.
- Initialize a local PostgreSQL cluster in `/opt/hams/pgdata` listening on `/opt/hams/pgsock`.
- Provision the necessary PostgreSQL roles (`odoo` and the current user).
- Run the full Odoo test suite sequentially.

## 2. Subsequent Runs (Fast Execution)

Once the Jules environment is successfully provisioned, you do not need to reinstall dependencies. Simply use the `--already-provisioned` flag:

```bash
IN_JULES_VM=1 python3 tools/test_runner.py --already-provisioned
```

This skips the APT operations and starts testing immediately, seamlessly connecting to the local database cluster at `/opt/hams/pgsock`.

You can append standard test runner arguments alongside this flag:

```bash
IN_JULES_VM=1 python3 tools/test_runner.py -m integration --already-provisioned
```

## 3. Targeting Specific Modules (-u flag)

By default, the test runner executes against all local modules. To restrict the testing scope to a single module (which saves significant time), use the `-u <module_name>` flag.

This flag works globally across **all** execution modes (`standard`, `integration`, `individual`, `xml`, `downloads`).

**Examples:**

Run the standard test suite but ONLY for the `user_websites` module:
```bash
IN_JULES_VM=1 python3 tools/test_runner.py -u user_websites --already-provisioned
```

Run integration tests specifically for `pager_duty`:
```bash
IN_JULES_VM=1 python3 tools/test_runner.py -m integration -u pager_duty --already-provisioned
```

Run the highly isolated individual test mode for `zero_sudo`:
```bash
IN_JULES_VM=1 python3 tools/test_runner.py -m individual -u zero_sudo --already-provisioned
```

## Note on Python Execution

When running the tests under `--provision-jules` or `--already-provisioned`, the system-installed Python (`/usr/bin/python3`) is utilized rather than the local `.venv`, ensuring that global Debian/Ubuntu python packages associated with the global Odoo 19 install remain accessible.

## 4. Handling Intermittent UI Tour Failures (Owl Rendering Delays)

> **NOTICE:** For the exhaustive, centralized guide on constructing resilient UI tours, you MUST consult [docs/LLM_WRITING_TOURS.md](LLM_WRITING_TOURS.md).

Due to resource constraints in the Jules VM and the asynchronous nature of Odoo 19's Owl UI framework, UI tours can suffer from race conditions where the tour executor attempts to click elements before they are fully rendered (especially modals and wizards).

To guarantee architectural compliance and stabilize the build, you MUST utilize the centralized DOM wait macros provided by `hams_test`.

**Import the Utilities:**
```javascript
import { TourUtils } from "@hams_test/js/tour_utils";
```

**Available Wait Macros:**
* `TourUtils.waitForAbsence(selector, description)`: Pauses the tour until the element is entirely removed from the DOM (e.g., waiting for an RPC loading overlay to vanish).

**Usage Example:**
```javascript
steps: () => [
    {
        content: "Click open wizard",
        trigger: 'button[name="open_wizard"]',
        run: 'click',
    },
    { trigger: '.modal-dialog', run: function() {} }, // Native DOM polling
    {
        content: "Interact with wizard",
        trigger: 'button[name="action_confirm"]',
        run: 'click',
    }
]
```
