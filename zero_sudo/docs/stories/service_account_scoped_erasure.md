<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Story: Service-Account-Scoped Erasure, Anonymization, and Account Integrity

This story describes how GDPR erasure/anonymization utilities avoid a real, previously-live bug class -- a service account whose own group memberships incidentally pick up an unrelated, restrictive `ir.rule`, silently narrowing its `search()` visibility below the real record set -- and how a handful of other small, independent account-integrity mechanisms fit alongside them.

## Ground Truth vs. a Service Account's Own Visibility
`_ground_truth_ids` `[@ANCHOR: zero_sudo:ground_truth_ids]` returns the literal set of ids matching a domain, bypassing both `ir.model.access` and `ir.rule` entirely via `Model._search(domain, bypass_access=True)` -- Odoo's own narrower, documented mechanism for this, not `.sudo()` (forbidden platform-wide). Used ONLY as a comparison baseline, never to perform the actual mutation.

## Erase and Anonymize, Impersonating the Real Service Account
`_erase_via_service_account` `[@ANCHOR: zero_sudo:erase_via_service_account]` hard-deletes every record matching a domain, acting as the named service account -- but first compares that account's own real `search()` visibility against the ground truth above. If they don't match (the account's own group memberships silently hid some of the real records from it), it raises an `AccessError` instead of silently deleting a partial subset. Found live in `ham_relay_bridge`: this exact mismatch made GDPR erasure of relay nodes a permanent, silent no-op until a test checked the actual outcome instead of just that the call didn't raise.

`_anonymize_via_service_account` `[@ANCHOR: zero_sudo:anonymize_via_service_account]` is the write-side sibling of the same pattern -- reassigning ownership rather than deleting, with the identical ground-truth-vs-visibility check and the identical fail-loud-not-silent posture.

## Service Account Password Integrity
`ResUsersZeroSudo.write` `[@ANCHOR: zero_sudo:res_users_write]` guarantees a service account's password can never be set to a caller-supplied value, even inside a batch write that also touches ordinary users -- a mixed recordset write is split so only the regular accounts in the batch receive the given password; any service account in the same batch still gets its own freshly forced-random one.

## Daemon Process Lifecycle
`_stop_daemon_process` `[@ANCHOR: zero_sudo:stop_daemon_process]` safely terminates a daemon subprocess this module started (SIGTERM, escalating to SIGKILL if it doesn't exit in time) -- the cleanup half of the daemon-management utilities `_start_daemon_process`/`_poll_health_check` already document elsewhere.

## Security Log Retention
`SecurityLog.autovacuum` `[@ANCHOR: zero_sudo:security_log_autovacuum]` purges security-log entries older than 90 days in bounded batches, run by its own dedicated cron under a narrowly-scoped facility service account (deliberately NOT ORM-writable by anything else, including its own cleanup job's ordinary caller -- see the cron's own `group_ids` grant, added specifically to authorize this one scheduled action without broadening the model's actual ACL).

## Is a Commit Safe Right Now?
`_is_test_mode` `[@ANCHOR: zero_sudo:is_test_mode]` reports whether `env.cr.commit()` would be unsafe or meaningless in the current context -- true inside both `HamsTransactionCase` (whose cursor patches `.commit` to raise) and `HamsHttpCase`-style tests (Odoo's own `TestCursor`), so callers that conditionally commit only in real, non-test execution have one real, direct signal to check instead of re-deriving it themselves.
