<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: Blocking Service Account Login `[@ANCHOR: zero_sudo:COMM_story_login_blocking]`

This story describes how the system prevents service accounts from being used for interactive web logins.

## Background
Service accounts are intended for background daemons and internal processes. They should never be used by humans to log into the Odoo web interface.

## The Process
1. **Account Flagging**: An administrator or a module's data file flags a user record as a service account using the `is_service_account` field `[@ANCHOR: zero_sudo:COMM_is_service_account_field]`.

This ensures that the user is officially recognized by the system as a background process rather than a human.

2. **Password Generation**: Upon creation, the system automatically assigns the service account a cryptographically secure, 128-byte random password. This guarantees that interactive authentication via standard credentials is mathematically impossible `[@ANCHOR: zero_sudo:COMM_service_account_password_generation]`.
3. **Login Attempt**: A user attempts to log into the web interface using the credentials of a service account.
4. **Interception**: The `web_login` interceptor `[@ANCHOR: zero_sudo:COMM_web_login_interceptor]` catches the successful authentication.

The interceptor runs before the session is fully established, preventing any unauthorized access to the backend.

5. **Security Check**: The system performs a direct SQL check `[@ANCHOR: zero_sudo:COMM_web_login_interceptor_check]` to verify the `is_service_account` flag.
6. **Session Destruction**: If the user is a service account, the system immediately destroys the session and redirects the user back to the login page with an error message.
7. **Security Logging**: The system records the blocked attempt in a centralized audit log (`zero_sudo.security.log`) for review by administrators `[@ANCHOR: zero_sudo:COMM_zero_sudo_security_log_global]`.

8. **Request-Level Guard**: Even if a service account somehow already has a live session (not obtained via `/web/login` itself -- e.g. a forged or otherwise-issued session cookie), `_authenticate()` `[@ANCHOR: zero_sudo:ir_http_authenticate]` re-checks `is_service_account` on every single authenticated request dispatch, not just at login time, and raises an `AccessError` for any request outside `/jsonrpc`/`/xmlrpc`.

## Security Benefit
This prevents an attacker who might have compromised a service account's credentials (e.g., from a config file) from using those credentials to access the Odoo backend UI.

## Verification
- **Automated Test**: `test_01_web_login_interceptor` in `test_controllers.py` verifies the blocking logic via HTTP POST.
- **UI Tour**: `zero_sudo_tour` `[@ANCHOR: zero_sudo:COMM_zero_sudo_tour]` verifies that a service account can be created and the flag is correctly handled in the UI.

## A Module Upgrade Must Not Spam Administrators About Accounts Nobody Logs Into

9. Filtering a recordset down to just the service accounts inside it is a plain SQL read, not an
   ORM search: `is_service_account` is `groups="base.group_system"`, so a narrower caller filtering
   through the ORM directly would raise `AccessError` on a field it has no business reading, even
   though it only needs to know "which of these users are service accounts", not the flag's actual
   value for any one of them `[@ANCHOR: zero_sudo:service_accounts_among_self]`.
10. A module upgrade rewrites a service account's own login/password fields on every declared data
    record, which would otherwise queue Odoo's own "Security Update: Login Changed"/"Password
    Changed" notification email per account, per upgrade -- a real account nobody ever logs into has
    no one to usefully receive that email, so it is suppressed specifically for service accounts,
    while an ordinary human user's own password-change notification is unaffected
    `[@ANCHOR: zero_sudo:service_account_security_notice_suppressed]`.
