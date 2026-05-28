# Jules VM Environment Issues - compliance

## Provisioning Issues
No errors encountered during `IN_JULES_VM=1 python3 tools/test.py --provision-jules`.

## Test Execution Issues
Standard tests failed for the `compliance` module.

### UI Tour Failure (Chrome Initialization)
When running `IN_JULES_VM=1 python3 tools/test.py -u compliance --already-provisioned`, the following error occurred:
```
2026-05-28 19:50:07,964 27686 WARNING hams_test odoo.addons.compliance.tests.test_ui_tours.TestComplianceUITour.test_compliance_tour: Chrome headless failed to start:
[27749:27772:0528/194958.566235:ERROR:dbus/bus.cc:405] Failed to connect to the bus: Address does not contain a colon
...
2026-05-28 19:50:07,980 27686 INFO hams_test odoo.addons.compliance.tests.test_ui_tours: skipped TestComplianceUITour.test_compliance_tour : Failed to detect chrome devtools port after 10.0s.
```
This indicates that Chrome Headless failed to initialize correctly within the Jules environment during the UI tour test.
