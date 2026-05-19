# Safe Mocking and Patching in Odoo

## The Problem
Odoo test environments are highly sensitive to module load order. Utilizing standard `@patch` decorators at the test method level forces Python to import the targeted models *before* Odoo's test runner has fully initialized the `self.env` registry. This bypasses the standard loading sequence, resulting in partial module registrations and downstream `KeyError` crashes (e.g., `KeyError: 'cloudflare.ip.ban'`).

Additionally, aggressive mocking of Odoo's internal ORM methods (like `search` or `write`) can easily create cyclic dependencies resulting in silent infinite loops that hang the CI pipeline.

## The Solution

We mandate the use of the `SafePatchMixin` and its `safe_patch` method, provided by `HamsTransactionCase` and `HamsHttpCase`.

### 1. `self.safe_patch(target)`
Instead of using `@patch` or `with patch():`, developers must invoke `self.safe_patch()` directly inside their test execution block or `setUp()` method.
This ensures the mock is instantiated *after* Odoo's registry is safely built, and uses `addCleanup()` to guarantee teardown without indentation nesting.

### 2. `DiagnosticMock`
By default, `self.safe_patch()` injects a `DiagnosticMock` instead of a standard `MagicMock`. The `DiagnosticMock` monitors the runtime call stack. If it detects rapid recursive execution exceeding a depth of 5, it raises an immediate, explicit `RecursionError` to fail the test quickly and clearly, preventing pipeline timeouts.

### Daemons and Testing Isolation
While standard architecture dictates that background daemons should be isolated from the Odoo framework, tests requiring extensive patching and mocking MUST inherit from `HamsIntegrationCase` (from `odoo.addons.hams_test.common`) instead of `unittest.TestCase`. This provides access to `self.safe_patch()`, resolving pipeline lockups without violating the underlying operational isolation of the daemon itself.

### Daemons and Testing Isolation
While standard architecture dictates that background daemons should be isolated from the Odoo framework, tests requiring extensive patching and mocking MUST inherit from `HamsIntegrationCase` (from `odoo.addons.hams_test.common`) instead of `unittest.TestCase`. This provides access to `self.safe_patch()`, resolving pipeline lockups without violating the underlying operational isolation of the daemon itself.

### Example Refactoring

**Bad (Early Import Crash):**
```python
@patch("odoo.addons.my_module.models.my_model.some_method")
def test_something(self, mock_method):
    mock_method.return_value = True
    ...
```

**Good (Runtime Safe):**
```python
def test_something(self):
    mock_method = self.safe_patch("odoo.addons.my_module.models.my_model.some_method")
    mock_method.return_value = True
    ...
```
