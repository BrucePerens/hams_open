# Jules Testing Issues - compliance module

## Provisioning
Provisioning completed successfully with `IN_JULES_VM=1 python3 tools/test.py --provision-jules`.

## Standard Tests
Run command: `IN_JULES_VM=1 python3 tools/test.py -u compliance --already-provisioned`

### Issues Encountered

#### 1. Chrome Headless Failure in UI Tours
The test `TestComplianceUITour.test_compliance_tour` was skipped because Chrome headless failed to start.

**Error log snippet:**
```
2026-05-29 02:17:40,381 22414 WARNING zero_sudo odoo.addons.compliance.tests.test_ui_tours.TestComplianceUITour.test_compliance_tour: Chrome headless failed to start:
[22477:22499:0529/021731.379354:ERROR:dbus/bus.cc:405] Failed to connect to the bus: Address does not contain a colon
[22513:22513:0529/021733.360689:WARNING:sandbox/policy/linux/sandbox_linux.cc:405] InitializeSandbox() called with multiple threads in process gpu-process.
[22477:22499:0529/021738.513517:ERROR:dbus/bus.cc:405] Failed to connect to the bus: Address does not contain a colon
[22477:22477:0529/021738.570453:INFO:components/enterprise/browser/controller/chrome_browser_cloud_management_controller.cc:225] No machine level policy manager exists.
[22477:22499:0529/021738.897853:ERROR:dbus/bus.cc:405] Failed to connect to the bus: Address does not contain a colon

2026-05-29 02:17:40,419 22414 INFO zero_sudo odoo.addons.compliance.tests.test_ui_tours: skipped TestComplianceUITour.test_compliance_tour : Failed to detect chrome devtools port after 10.0s.
```

**Analysis:**
This appears to be an environment issue within the Jules VM where Chrome cannot properly initialize in headless mode, possibly due to missing D-Bus connection or sandbox restrictions. This prevented the UI tour from running.
