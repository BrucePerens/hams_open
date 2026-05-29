# Jules Issues for Compliance Module

## Automated Review Results

### 1. AI Hallucination & Laziness
- Found `hasattr` checks in `hooks.py` designed to bypass missing methods or fields:
    - `if "cookies_bar" in env_svc["website"]._fields:` in `post_init_hook`.
    - `if page.view_id and page.view_id.key and page.view_id.key.startswith("compliance.compliance_"):` in `is_boilerplate`.
- Found `self.assertTrue(1 == 1)` or similar always-true assertions: None found.
- Found empty `except:` blocks: None found.

### 2. Fallbacks & Missing Resources
- No inline OS package installations found.
- Loading order seems correct.

### 3. Zero-Sudo & Micro-Privilege
- No `.sudo()` calls found.
- Usage of `env["zero_sudo.security.utils"]._get_service_env("compliance.user_compliance_service")` is correct and follows ADR-0002.

### 4. Multi-Tenant Awareness
- `post_init_hook` handles multiple websites.
- `Website` model inheritance sets `cookies_bar` default.
- `_register_hook` calls `_bootstrap_knowledge_docs` which is global.

### 5. Security Audit
- ACLs in `ir.model.access.csv` are minimal.
- Usage of service accounts follows best practices.

### 6. Documentation
- `README.md` and `data/documentation.html` are present and detailed.

### 7. Semantic Anchors
- Semantic anchors are present and verified by linters.

### 8. Test Coverage
- Standard tests, integration tests, and UI tours are present.

## Identified Improvements
- Remove redundant `hasattr` or field presence checks that are too defensive and should fail fast if the environment is misconfigured.
- Ensure all UI tours utilize `TourUtils.waitForElement` where appropriate.
- Ensure all Python test files invoking `self.start_tour` append `?debug=1`. (Already done in `test_ui_tours.py`).

## Proposed Linter Rules for `check_burn_list.py`

To catch AI laziness and ensure "Fail Fast" compliance, I propose the following AST-based linter rules:

1. **Anti-Defensive Odoo Hook Rule**: Flag usage of field existence checks (e.g., `if "field_name" in model._fields:`) within `post_init_hook` or `_register_hook`. Hooks should assume the schema is correct and fail loudly if dependencies are missing.
2. **Anti-hasattr Guard Rule**: Flag usage of `hasattr(record, 'field_or_method')` when used as a defensive guard around core logic. If a record is expected to have a property, the code should access it directly.
3. **Implicit Singleton Rule**: Flag `model.write(...)` inside a loop over a recordset if the recordset is already known to be filtered. Propose using `recordset.write(...)` directly unless Odoo specific constraints require singletons.
