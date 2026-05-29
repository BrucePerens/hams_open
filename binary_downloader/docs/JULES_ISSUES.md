# Jules Session Issues - binary_downloader

## Environment Verification
- **Date:** 2026-05-29
- **Provisioning:** Successful using `IN_JULES_VM=1 python3 tools/test.py --provision-jules -u binary_downloader`.
- **Standard Tests:** Passed (16 tests).
- **UI Tours:** Passed as part of the test suite.

## Repaired AI Hallucinations & Laziness
- **Empty Exception Handler:** In `models/binary_manifest.py`, an empty `except OSError: pass` block was found in the cleanup logic. This was repaired by adding a warning log (`_logger.warning`) to ensure that failures to delete temporary files are visible in the logs.
- **UI Visibility Logic:** In `views/binary_manifest_views.xml`, the `extract_member` field was incorrectly hidden for ZIP archives. This appeared to be an AI shortcut that assumed only tarballs need member extraction. Fixed to show the field for both `tar.gz` and `zip` types.

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

## Identified Issues
- **Global Regression Failure (Unrelated):** `manual_library` fails in `test_04_parent_deletion_restriction` due to a `psycopg2.errors.RestrictViolation`. This appears to be an existing issue in the codebase and is not related to `binary_downloader`.
