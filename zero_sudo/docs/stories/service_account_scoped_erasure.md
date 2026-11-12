<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: Service-Account-Scoped Erasure and Account Integrity

This story describes how the GDPR erasure utility avoids a real failure mode -- a service account whose own group memberships incidentally pick up an unrelated, restrictive `ir.rule`, silently narrowing its `search()` visibility below the real record set -- and how a handful of other small, independent account-integrity mechanisms fit alongside them.

## Ground Truth vs. a Service Account's Own Visibility
`_ground_truth_ids` `[@ANCHOR: zero_sudo:ground_truth_ids]` returns the literal set of ids matching a domain, bypassing both `ir.model.access` and `ir.rule` entirely via `Model._search(domain, bypass_access=True)` -- Odoo's own narrower, documented mechanism for this, not `.sudo()` (forbidden platform-wide). Used ONLY as a comparison baseline, never to perform the actual mutation.

## Erase, Impersonating the Real Service Account
`_erase_via_service_account` `[@ANCHOR: zero_sudo:erase_via_service_account]` hard-deletes every record matching a domain, acting as the named service account -- but first compares that account's own real `search()` visibility against the ground truth above. If they don't match (the account's own group memberships silently hid some of the real records from it), it raises an `AccessError` instead of silently deleting a partial subset. Found live in `ham_relay_bridge`: this exact mismatch made GDPR erasure of relay nodes a permanent, silent no-op until a test checked the actual outcome instead of just that the call didn't raise.

## Service Account Password Integrity
`ResUsersZeroSudo.write` `[@ANCHOR: zero_sudo:res_users_write]` guarantees a service account's password can never be set to a caller-supplied value, even inside a batch write that also touches ordinary users -- a mixed recordset write is split so only the regular accounts in the batch receive the given password; any service account in the same batch still gets its own freshly forced-random one.

## Daemon Process Lifecycle
### `_stop_daemon_process(process)` `[@ANCHOR: zero_sudo:stop_daemon_process]`

| Argument | Type | Meaning |
|---|---|---|
| `process` | `subprocess.Popen` or `None` | a daemon this module started with `_start_daemon_process`, which starts it in its own session |

`@api.model`; returns nothing. Does nothing for `None` or a process that has already exited (`poll()` is not `None`). Otherwise it
asks the daemon's whole process group to terminate (SIGTERM, so child processes go too) and waits up to 5 seconds; if the daemon
is still running it logs a warning and forces the group to stop (SIGKILL). A process that exits between the `poll()` check and a
signal (`ProcessLookupError`) counts as success, in both steps, rather than an error. It signals only the process group of the
`Popen` object it is given, never a pid found by searching. No database access and no lock. Other `OSError`s from the system
propagate. It is the cleanup half of `_start_daemon_process` and `_poll_health_check`, which are documented in the README.

## Is a Commit Safe Right Now?
`_is_test_mode` `[@ANCHOR: zero_sudo:is_test_mode]` reports whether `env.cr.commit()` would be unsafe or meaningless in the current context -- true inside both `HamsTransactionCase` (whose cursor patches `.commit` to raise) and `HamsHttpCase`-style tests (Odoo's own `TestCursor`), so callers that conditionally commit only in real, non-test execution have one real, direct signal to check instead of re-deriving it themselves.

## Removed 2026-10-11: security-log autovacuum and ownership anonymization
Bruce Perens, 2026-10-11: "We retain data forever" and "Completely remove any code for logbook anonymization." Two mechanisms that used to be described in this story no longer exist.
* The security-log autovacuum and its cron (`zero_sudo.ir_cron_security_log_autovacuum`) are gone. `zero_sudo.security.log` rows are kept forever. zero_sudo 1.1's migration unlinks the cron from installed databases.
* The ownership-anonymizing sibling of erasure is gone. Erasure no longer reassigns anything to a placeholder owner. The `zero_sudo.orphaned_record_owner` account is retained only because rows reassigned to it before this decision still point at it.
* `_erase_via_service_account` and `_ground_truth_ids` are unchanged.
