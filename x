# Issues encountered in Jules environment

1. The `hams_community` repository was extracted to `/app/hams_community`.
2. When running tests on `ham_base`, the AST Burn List Linter failed with the following error:
```
 📄 tests/test_mro_architecture.py
  ❌ ERROR: Line 55 (AST): CRITICAL AI LAZINESS: Empty exception handlers using 'pass' are forbidden. Log the error or handle it.
      Code: `except StopIteration:`
```

