# JULES_ISSUES.md for caching module

## Environment Verification
- Provisioning started at Sat May 30 18:03:06 UTC 2026

## Test Failures
- **Date**: Sat May 30 18:05:05 UTC 2026
- **Test**: `TestCachingTour.test_caching_service_worker_tour`
- **Error**: `skipped TestCachingTour.test_caching_service_worker_tour : websocket-client module is not installed`
- **Traceback**:
```
2026-05-30 18:04:49,930 19389 WARNING zero_sudo odoo.addons.caching.tests.test_tour.TestCachingTour.test_caching_service_worker_tour: websocket-client module is not installed
2026-05-30 18:04:49,930 19389 WARNING zero_sudo odoo.addons.zero_sudo.tests.common: TRACING: Chrome init failed on attempt 1 (websocket-client module is not installed). Retrying...
```
- **Resolution**: Environment limitation. UI tours cannot be executed due to missing `websocket-client` in the Jules VM environment. Per instructions, I am forbidden from creating or modifying UI tours.

## AI Hallucination & Laziness
- Found in `caching/tests/test_settings_and_cache.py`:
  ```python
  def test_03_caching_sudo_params(self):
      # ...
      self.assertTrue(val is not None or val is None)
  ```
  This assertion is always true and provides no value. It will be removed or replaced with a meaningful assertion.

## Final Verification
- **Date**: Sat May 30 18:07:02 UTC 2026
- **Status**: Standard tests pass. UI tours are skipped due to environment limitations (missing `websocket-client`).
- **Conclusion**: Proceeding with module review and repair under VM LIMITATION PROTOCOL.

## Linter Suggestions for check_burn_list.py
- **Always True Assertions**: Add a rule to detect `assertTrue(X or not X)` or `assertTrue(is None or is not None)`.
- **Trivial Assertions**: Flag assertions that are statistically likely to be AI shortcuts, such as `self.assertTrue(1 == 1)`.
