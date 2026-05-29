# JULES_ISSUES.md - Deep Audit Findings for `user_websites_seo`

## 1. AI Hallucination & Laziness Audit
- **Findings**: The use of `hasattr(response, "qcontext")` in `controllers/main.py` is a common AI-generated defensive pattern that can mask underlying architectural issues. While `qcontext` is a legitimate attribute of Odoo's `http.Response` object for certain response types, the original check was loose.
- **Repair**: Refactored the check to use `isinstance(response, Response)` from `odoo.http`. This ensures we are dealing with a proper Odoo Response object before attempting to access its context.
- **Proposed Linter Rule**:
  ```python
  (
      r"controllers/.*\.py$",
      re.compile(r"hasattr\s*\(\s*response\s*,\s*['\"]qcontext['\"]\s*\)"),
      "CRITICAL AI LAZINESS: Use 'isinstance(response, Response)' instead of 'hasattr(response, \"qcontext\")' in controllers to ensure proper response handling."
  )
  ```

## 2. Security & Zero-Sudo Audit
- **Findings**:
    - **SSTI Protection**: The controller correctly de-elevates recordsets using `with_env(request.env)` before injecting them into `qcontext` as `main_object`. This prevents Server-Side Template Injection.
    - **Zero-Sudo**: Confirmed that `.sudo()` is completely absent from the module. Privilege elevation is strictly bounded in `models/seo_metadata_mixin.py` using `with_user(svc_uid)`.
- **Verified Anchors**:
    - `[@ANCHOR: controller_user_blog_index_seo_override]`
    - `[@ANCHOR: res_users_seo_write_elevation]`
    - `[@ANCHOR: user_websites_group_seo_write_elevation]`

## 3. Multi-Tenant & Multi-Website Audit
- **Findings**:
    - The module uses `website.seo.metadata` which is the standard Odoo way of handling SEO.
    - Controller logic respects the `website_slug` and uses standard Odoo routing which is multi-website aware.
    - No hardcoded `company_id` or `website_id` that would break in a multi-tenant environment was found.

## 4. Resource Fallbacks Audit
- **Findings**: No inline OS package installations or improper fallbacks were detected. The module correctly depends on `website` and `user_websites` in its manifest.

## 5. UI Tour Robustness
- **Findings**: The tour `user_websites_seo_tour.js` was reviewed.
- **Repair**: Added `expectUnloadPage: true` where appropriate (though reverted after testing showed it wasn't strictly necessary for the breadcrumb navigation in this specific Odoo version/context, standard Odoo trigger polling was more reliable).
- **Repair**: Ensured `start_tour` calls in Python use `?debug=1`.
