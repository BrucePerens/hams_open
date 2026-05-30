# Jules Session Issues - binary_downloader

## Environment Verification
- **Date:** 2026-05-31
- **Status:** Review completed.
- **Provisioning:** Successful. Standard tests and UI tours pass in Jules VM.
- **Linter/Anchor Issues:** Anchor linter reported duplicates due to provisioning artifacts in `hams_community/`; this is environmental and not a module bug.

## AI Hallucination & Laziness Audit
- **Identified Shortcuts:**
    - `binary_version.py` was missing ZIP symlink protection (Tarball had it, ZIP didn't). FIXED.
    - `pager_integration.py` used a soft fallback (`return False`) when the `pager_duty` module was missing instead of failing fast. FIXED to `UserError`.
    - `binary_manifest.py` had a multi-tenant leak where it could fall back to another company's binary manifest if the current company didn't have one. FIXED to only fall back to global manifests (`company_id = False`).
- **Proposed Linter Rules for `check_burn_list.py`:**
    - `ban_soft_fallback_on_missing_model`: Flag `if 'model.name' not in self.env: return False` patterns. Encourage `UserError` or proper dependency management.
    - `require_zip_symlink_check`: Ensure any `zipfile.ZipFile` extraction includes a check for `external_attr` symlink bits.
    - `strict_multi_tenant_search`: Flag `search()` calls in multi-tenant models that don't explicitly filter by `company_id` or `False`.

## Multi-Tenant Awareness
- **Decision:** `binary.manifest` and `binary.version` are multi-tenant by default (have `company_id`).
- **Global Fallback:** `binary.manifest` now supports optional `company_id`. If `company_id` is empty, it's considered a "Global" manifest provided by the system.
- **Isolation:** `ensure_executable` now strictly respects the company hierarchy: Current Company -> Global. It will NO LONGER use a manifest belonging to a different company.

## Security Audit
- **ZIP/Tar Slip:** Verified and tested.
- **Symlinks:** Added explicit protection for ZIP archives in `binary_version.py` (matching existing `binary_manifest.py` logic).
- **Micro-Privilege:** Confirmed all system-level operations use `user_binary_downloader_service` via `with_user()`.
