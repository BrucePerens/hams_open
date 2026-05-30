# JULES_ISSUES.md - hams_helpdesk

## Environment Verification
- **Status**: SUCCESS
- **Timestamp**: 2026-05-30 18:08:00 UTC
- **Notes**: Environment provisioned successfully. Standard tests passed using `tools/test.py -u hams_helpdesk`.

## AI Hallucinations & Laziness
- **Repaired**: Removed `hasattr` check in `helpdesk_ticket.py`. Replaced with check for field existence in `Calendar._fields` which is a more robust way to detect if another module has extended the model.
- **Audit**: Verified tests don't use tautological assertions like `assertTrue(1 == 1)`.

## Fallbacks & Missing Resources
- TBD

## Zero-Sudo & Micro-Privilege
- **Audit**: Confirmed zero usage of `.sudo()`.
- **Audit**: All background/elevated tasks use `with_env(service_env)` via `zero_sudo.security.utils`.
- **Security Audit**: Verified that portal users have restricted write access via `write` override in `helpdesk_ticket.py`.

## Multi-Tenant Awareness
- **Repaired**: Added `company_id` to `hams_helpdesk.ticket` and implemented multi-company record rules.
- **Improved**: `create` method now automatically infers `company_id` from `website_id` if provided.

## Security Audit
- **Status**: PASSED. Verified IR rules, portal restrictions, and zero-sudo compliance.

## Documentation
- **Status**: UPDATED. README.md and documentation.html reviewed and updated.

## Semantic Anchors
- **Status**: VERIFIED. All anchors in code are referenced in README.md and tested in test suites. Fixed a missing anchor in `helpdesk_ticket.py`.
