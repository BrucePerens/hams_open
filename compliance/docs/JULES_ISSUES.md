# JULES ISSUES - compliance module

## Environment Verification
- Provisioning: Success
- Standard Tests: Success
- UI Tours: Success
- Linter: Success

The environment is fully operational for the `compliance` module.

## AI Hallucination & Laziness Repairs
- **Repair**: Removed defensive `hasattr` or `_fields` checks in `compliance/tests/test_hooks.py`.
- **Reasoning**: Instruction mandate to FAIL FAST. Tests should not skip if the expected schema is missing; they should fail to signal a regression or misconfiguration.
- **Proposed Linter Rule**: Add a check to `check_burn_list.py` that forbids `SkipTest` or `hasattr` checks when they are used to bypass missing fields on core models like `website`.

## Multi-Tenant Awareness
- All models and hooks audited. The `post_init_hook` correctly handles all websites by iterating over them and using `env_svc` with `active_test=False`.
- `cookies_bar` field on `website` model has a default value of `True` to ensure compliance for new websites.
