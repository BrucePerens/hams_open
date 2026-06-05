# JULES_ISSUES.md - binary_downloader

## Findings from Deep Review & Security Audit

1. **Missing URL Scheme Validation in `binary.version`:** The `binary.version` model lacks the `http://` and `https://` scheme validation present in `binary.manifest`. This could allow non-HTTP URLs if not properly constrained.
2. **Deterministic Locking in `binary.manifest`:** The locking mechanism in `ensure_executable` is good, but I should verify if it covers all cases where concurrent downloads might occur.
3. **Symlink Security in `binary.tenant_link`:** The symlink creation uses absolute paths. While it's within `data_dir`, we should ensure it cannot be exploited to point outside. The current implementation seems to confine it to `tenant_bins`.
4. **Diagnostic Messages:** Adding `[!] DIAGNOSTIC FOR AI:` to all assertions in tests to improve failure analysis.
5. **Multi-tenant isolation:** `binary.manifest` uses `company_id`, while `binary.tenant.link` uses `website_id`. This is consistent with Odoo's multi-company/multi-website architecture.
