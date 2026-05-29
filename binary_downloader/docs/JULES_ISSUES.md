# Jules Session Issues - binary_downloader

## Environment Verification
- **Date:** 2026-05-29
- **Provisioning:** Successful using `IN_JULES_VM=1 python3 tools/test.py --provision-jules -u binary_downloader`.
- **Standard Tests:** Passed (16 tests).
- **UI Tours:** Passed as part of the test suite.

## Repaired AI Hallucinations & Laziness
- **Empty Exception Handler:** In `models/binary_manifest.py`, `tests/test_binary_manifest.py`, and `tests/test_ui_tours_api.py`, empty `except OSError: pass` blocks were found. These were repaired by adding proper `_logger.warning` calls to ensure that failures to delete temporary or test files are visible in the logs.
- **UI Visibility Logic:** In `views/binary_manifest_views.xml`, the `extract_member` field was incorrectly hidden for ZIP archives. This was an AI shortcut assuming only tarballs need member extraction. Fixed to show for both `tar.gz` and `zip`.
- **UI Tour Robustness:** Updated `static/tests/tours/binary_install_tour.js` to use `TourUtils.waitForAbsence('.o_loading')` and added explicit waits for notifications to prevent race conditions during form submission and RPC resolution.

## Proposed Linter Rules
To catch empty exception handlers globally, I propose adding the following AST check to `tools/check_burn_list.py`:

```python
def visit_ExceptHandler(self, node):
    if len(node.body) == 1 and isinstance(node.body[0], ast.Pass):
        self.add_error(
            node.lineno,
            "CRITICAL AI LAZINESS: Empty exception handlers using 'pass' are forbidden. Log the error or handle it."
        )
    self.generic_visit(node)
```

## Security Audit
- **Zero-Sudo Compliance:** Verified. No `.sudo()` calls found. Service account `user_binary_downloader_service` is correctly used with `.with_user()`.
- **Path Traversal:** Protections against Tar Slip and Zip Slip are verified by tests.
- **Multi-Tenant Awareness:** Binaries are stored in a shared system directory (`hams_bin`), but their orchestration and access are governed by Odoo's standard security model, ensuring isolation at the service level.
