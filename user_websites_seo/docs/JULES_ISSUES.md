# JULES_ISSUES.md - Deep Audit Findings for `user_websites_seo`

## VM Verification
- **Status**: Verified.
- **Timestamp**: 2026-05-30 18:15 UTC
- **Observations**: Environment provisioned using `tools/test.py --provision-jules`. Standard tests for `user_websites_seo` passed successfully.
- **UI Tours**: Headless Chrome is functional. `user_websites_seo_tour` passed.

## 1. AI Hallucination & Laziness Audit
- **Findings**: The use of `hasattr(response, "qcontext")` in `controllers/main.py` is a common AI-generated defensive pattern.
- **Repair**: Refactored to use `isinstance(response, Response)` and `getattr(response, "qcontext", None)` to safely handle the response object without masking potential issues.
- **Proposed Linter Rule**:
  ```python
  (
      r"controllers/.*\.py$",
      re.compile(r"hasattr\s*\(\s*response\s*,\s*['\"]qcontext['\"]\s*\)"),
      "CRITICAL AI LAZINESS: Use 'isinstance(response, Response)' and check if qcontext is not None instead of 'hasattr(response, \"qcontext\")' in controllers."
  )
  ```

## 2. Fallbacks & Missing Resources Audit
- **Findings**: No inline OS package installations or improper fallbacks were detected. The module correctly uses the manifest to manage dependencies and utilizes `zero_sudo` for soft dependency handling (documentation installation).
- **FAIL FAST**: Verified that the module relies on Odoo's standard dependency management which fails fast if a required module is missing.

## 3. Zero-Sudo & Micro-Privilege Audit
- **Findings**:
    - Confirmed that `.sudo()` is completely absent from the module.
    - Privilege elevation is strictly bounded in `models/seo_metadata_mixin.py` using `with_user(svc_uid)`.
    - Service account `user_websites.user_websites_service_account` is used for SEO writes, which is consistent with the `user_websites` module architecture.
    - Controller `user_blog_index` correctly de-elevates records using `with_env(request.env)` to prevent SSTI.

## 4. Multi-Tenant & Multi-Website Awareness Audit
- **Findings**:
    - The module inherits from `res.users` and `user.websites.group`, both of which are multi-tenant.
    - SEO metadata is stored on the records themselves, respecting the standard Odoo record-level isolation.
    - Added detailed comments to models justifying their multi-tenant nature.
    - Controller logic utilizes standard Odoo routing and `website=True`, which is multi-website aware.

## 5. Security Audit
- **Findings**:
    - **SSTI Mitigation**: As noted in Zero-Sudo audit, the controller de-elevates recordsets.
    - **Access Control**: `_check_seo_write_permission` ensures users can only edit their own SEO data or groups they belong to.
    - **Injection**: Standard Odoo ORM methods are used, mitigating SQL injection.
    - **IDOR**: Access to SEO fields is checked against the current user's identity or group membership.

## 6. Documentation Audit
- **Findings**:
    - `README.md` and `data/documentation.html` were reviewed and found to be comprehensive.
    - Updated `README.md` to explicitly mention Multi-Tenant and Multi-Website support.
    - Ensured Zero-Sudo architecture is explained in plain English for end-users.

## 7. Semantic Anchor Audit
- **Findings**:
    - All semantic anchors `[@ANCHOR: ...]` are correctly cross-referenced between models, controllers, and tests.
    - Added missing `[@ANCHOR: test_seo_widget_tour]` to `tests/test_seo_ui_tour.py`.
    - Verified parity between source and test anchors.
