<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: Secure Privilege Escalation `[@ANCHOR: zero_sudo:COMM_story_secure_escalation]`

This story describes how developers securely escalate privileges without using the dangerous `.sudo()` command.

## Background
In the Zero-Sudo architecture, direct use of `.sudo()` is prohibited. Instead, background tasks must be executed by specific service accounts.

## The Process
1. **Developer Identification**: The developer identifies a need for elevated privileges (e.g., creating a record that the current user shouldn't have access to).
2. **Service Account Retrieval**: The developer uses the `_get_service_uid` function `[@ANCHOR: zero_sudo:COMM_get_service_uid]` to get the ID of a pre-defined service account.
3. **Impersonation**: The developer uses `.with_user(svc_uid)` to execute the specific operation as the service account.

## Security Enforcement
The `_get_service_uid` function ensures:
- The account exists and is resolved via raw SQL `[@ANCHOR: zero_sudo:COMM_get_service_uid_sql_resolve]`.

This direct SQL resolution prevents ORM-based interception or caching issues.

- The account is active and verified as a service account `[@ANCHOR: zero_sudo:COMM_get_service_uid_sql_verify]`.

This guarantees that deactivated accounts cannot be maliciously revived for background tasks.

- The account is explicitly flagged as a service account `[@ANCHOR: zero_sudo:COMM_is_service_account_field]`.

Only officially designated service accounts are allowed to proceed.

- The account DOES NOT have global administrative privileges (like `base.group_system`) `[@ANCHOR: zero_sudo:COMM_privilege_escalation_block_sql]`.

## Example
```python
svc_uid = self.env['zero_sudo.security.utils']._get_service_uid('my_module.my_service_user')
self.env['my.model'].with_user(svc_uid).create({'name': 'Secure Record'})
```

## A Whole Environment, Not Just a UID
Some callers need more than a bare uid -- `_get_service_env` `[@ANCHOR: zero_sudo:get_service_env]` returns a full `Environment` already running as the named service account, with `mail_notrack` forced on so background service-account writes don't spam chatter/followers the way an interactive user's own writes would.

## Fetching an Executable as a Service Account
`_ensure_executable` `[@ANCHOR: zero_sudo:ensure_executable]` resolves a system binary a daemon needs (e.g. `kopia` for backups): if it's already on `PATH`, use it directly; if not, fall back to a service-account-scoped binary-manifest downloader rather than failing outright or silently degrading.

## Declared Cross-Module Dependencies, Without a Hard `depends`
Some modules need to call into another module's own code only when that OTHER module happens to be installed, without adding a hard manifest dependency that would close a real dependency cycle (see each such module's own `depends_cycle` manifest key). `_resolve_dependency_cycle` `[@ANCHOR: zero_sudo:resolve_dependency_cycle]` checks that the CALLING module's own manifest actually declares this relationship (resolved from the real Python call stack via `_caller_module_name` `[@ANCHOR: zero_sudo:caller_module_name]`, not a caller-supplied string that could be used to probe an arbitrary undeclared module) before reporting whether the target module is installed -- raising if `required=True` and it isn't.
