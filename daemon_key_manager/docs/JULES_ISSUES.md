# Issues identified in daemon_key_manager

## 1. Test Failure: TestKeyRegistryTour.test_daemon_key_manager_tour
The UI tour test failed because Chrome headless could not start correctly in the Jules VM environment.

**Error Message:**
```
Chrome headless failed to start:
[19304:19327:0529/013245.489192:ERROR:dbus/bus.cc:405] Failed to connect to the bus: Address does not contain a colon
[19304:19304:0529/013254.717061:ERROR:dbus/object_proxy.cc:572] Failed to call method: org.freedesktop.DBus.NameHasOwner: object_path= /org/freedesktop/DBus: unknown error type:
...
skipped TestKeyRegistryTour.test_daemon_key_manager_tour : Failed to detect chrome devtools port after 10.0s.
```

**Context:**
The test runner reports this as a failure at the end of the run despite the "skipped" message in the log for the specific tour, likely because the Chrome process itself crashed or timed out during initialization.

## 2. Test Skipped: TestKeyRegistry.test_documentation_installed
The test verifying the installation of documentation was skipped because the required models (`knowledge.article` or `manual.article`) were not present in the environment.

**Log Message:**
```
skipped TestKeyRegistry.test_documentation_installed : No documentation model available
```

**Context:**
This suggests that the `knowledge` or `manual_library` modules (which would provide these models) are not installed or included in the test database schema by default when running tests for `daemon_key_manager`.
