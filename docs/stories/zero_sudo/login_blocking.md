# Story: Blocking Service Account Login `[@ANCHOR: story_login_blocking]`

This story describes how the system prevents service accounts from being used for interactive web logins.

## Background
Service accounts are intended for background daemons and internal processes. They should never be used by humans to log into the Odoo web interface.

## The Process
1. **Account Flagging**: An administrator or a module's data file flags a user record as a service account using the `is_service_account` field `[@ANCHOR: is_service_account_field]`.
2. **Login Attempt**: A user attempts to log into the web interface using the credentials of a service account.
3. **Interception**: The `web_login` interceptor `[@ANCHOR: web_login_interceptor]` catches the successful authentication.
4. **Security Check**: The system performs a direct SQL check `[@ANCHOR: web_login_interceptor_check]` to verify the `is_service_account` flag.
5. **Session Destruction**: If the user is a service account, the system immediately destroys the session and redirects the user back to the login page with an error message.

## Security Benefit
This prevents an attacker who might have compromised a service account's credentials (e.g., from a config file) from using those credentials to access the Odoo backend UI.

## Verification
- **Automated Test**: `test_01_web_login_interceptor` in `test_controllers.py` verifies the blocking logic via HTTP POST.
- **UI Tour**: `zero_sudo_tour` `[@ANCHOR: zero_sudo_tour]` verifies that a service account can be created and the flag is correctly handled in the UI.
