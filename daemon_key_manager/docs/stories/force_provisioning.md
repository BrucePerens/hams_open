# Story: Manual Force Provisioning

As a **DevOps Engineer** setting up a new environment,
I want to **force the generation of all daemon keys immediately**,
so that my external containers can start up without waiting for the next cron cycle.

## Scenario: Infrastructure Bootstrapping

1.  The automated deployment script (e.g., Ansible or a Kubernetes init container) finishes installing Odoo and its modules.
2.  The script executes a shell command to force provisioning:
    ```bash
    odoo-bin shell ... -e "env['daemon.key.registry'].action_force_provision_all(); env.cr.commit()"
    ```
3.  The `daemon_key_manager` iterates through all registered daemons and regenerates their keys and files.
4.  Once the command completes, all `.env` files are guaranteed to be present and valid.
5.  The dependent daemon containers are then started, find their keys, and connect to Odoo immediately.

## Error Handling
- If a file cannot be written due to permission issues (e.g., the volume isn't mounted correctly), an informative `UserError` is raised.

## Removed Daemons Are Skipped, Not Failures
- A registry row whose service account has been archived belongs to a daemon that was removed (the wildcard-certificate renewal was the first). Force provisioning skips it, logs one line naming the skipped daemons, and carries on, so a removed daemon does not make `hams.daemon.keys.service` exit non-zero after every run and hide a real failure behind a permanently red unit [@ANCHOR: COMM_force_provision_skips_archived].

## Entries

### `action_force_provision_all`: failures are collected, not raised [@ANCHOR: COMM_force_provision_error_handling]

Each registry is provisioned inside its own savepoint, so a failure (`UserError`, `ValidationError`, `AccessError`, `OSError`, for example a key volume that is not mounted or has the wrong permissions) is logged and undoes only that registry's database changes. The others still succeed. At the end, if any failed, the method returns a sticky danger notification naming them and the number that worked, rather than raising, because raising would roll back the rotations that had already written their key files and leave files and database disagreeing. The bootstrap script treats a non-success result as a failed unit. Tests: `test_force_provisioning`, `test_a_failing_daemon_does_not_undo_or_hide_the_daemons_that_succeeded`.

### `DaemonKeyRegistry.action_force_provision_all(self)` [@ANCHOR: COMM_action_force_provision_all_api]

Synchronously provisions keys for all registered daemons; meant for `odoo-bin shell` at deployment so key files exist before the daemons start. No arguments. Only the superuser or a Daemon Key Manager may call it (`AccessError` otherwise); it then runs as the manager service account and exposes itself with `@public_rpc`. Returns a client notification: success, or a sticky danger notice listing the daemons that failed. The caller commits afterwards. Rotating every key also revokes every old key, so daemons re-read their files. Tests: `test_force_provisioning`, `test_unauthorized_access`.

### `action_force_provision_all`: which registries and how [@ANCHOR: COMM_force_provision_logic]

Selects up to 1000 registries that are not remote self-rotating (re-provisioning those would revoke the key the remote daemon holds), skips and logs those whose service account is archived, fetches the old keys of the rest in one query, and rotates each with `_rotate_key_and_write_file` in its own savepoint and its own company. Every registry is attempted even when an earlier one fails; failures are collected and reported at the end rather than aborting the batch, so one broken key file cannot stop the other daemons from being provisioned. Tests: `test_force_provisioning`, `test_a_failing_daemon_does_not_undo_or_hide_the_daemons_that_succeeded`.
