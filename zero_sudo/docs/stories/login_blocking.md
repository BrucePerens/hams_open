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
2. **Password Generation**: Upon creation, the system automatically assigns the service account a cryptographically secure, 128-byte random password. This guarantees that interactive authentication via standard credentials is mathematically impossible `[@ANCHOR: zero_sudo:COMM_service_account_password_generation]`.
3. **Login Attempt**: A user attempts to log into the web interface using the credentials of a service account.
4. **Interception**: The `web_login` interceptor `[@ANCHOR: zero_sudo:COMM_web_login_interceptor]` catches the successful authentication.
5. **Security Check**: The system performs a direct SQL check `[@ANCHOR: zero_sudo:COMM_web_login_interceptor_check]` to verify the `is_service_account` flag.
6. **Session Destruction**: If the user is a service account, the system immediately destroys the session and redirects the user back to the login page with an error message.
7. **Security Logging**: The system records the blocked attempt in a centralized audit log (`zero_sudo.security.log`) for review by administrators `[@ANCHOR: zero_sudo:COMM_zero_sudo_security_log_global]`.

## Security Benefit
This prevents an attacker who might have compromised a service account's credentials (e.g., from a config file) from using those credentials to access the Odoo backend UI.

## Verification
- **Automated Test**: `test_01_web_login_interceptor` in `test_controllers.py` verifies the blocking logic via HTTP POST.
- **UI Tour**: `zero_sudo_tour` `[@ANCHOR: zero_sudo:COMM_zero_sudo_tour]` verifies that a service account can be created and the flag is correctly handled in the UI.
