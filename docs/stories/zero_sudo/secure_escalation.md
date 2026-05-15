# Story: Secure Privilege Escalation `[@ANCHOR: story_secure_escalation]`

This story describes how developers securely escalate privileges without using the dangerous `.sudo()` command.

## Background
In the Zero-Sudo architecture, direct use of `.sudo()` is prohibited. Instead, background tasks must be executed by specific service accounts.

## The Process
1. **Developer Identification**: The developer identifies a need for elevated privileges (e.g., creating a record that the current user shouldn't have access to).
2. **Service Account Retrieval**: The developer uses the `_get_service_uid` function `[@ANCHOR: get_service_uid]` to get the ID of a pre-defined service account.
3. **Impersonation**: The developer uses `.with_user(svc_uid)` to execute the specific operation as the service account.

## Security Enforcement
The `_get_service_uid` function ensures:
- The account exists.
- The account is active.
- The account is explicitly flagged as a service account `[@ANCHOR: is_service_account_field]`.
- The account DOES NOT have global administrative privileges (like `base.group_system`).

## Example
```python
svc_uid = self.env['zero_sudo.security.utils']._get_service_uid('my_module.my_service_user')
self.env['my.model'].with_user(svc_uid).create({'name': 'Secure Record'})
```
