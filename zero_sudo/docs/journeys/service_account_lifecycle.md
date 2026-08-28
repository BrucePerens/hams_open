<!--
Copyright (c) Bruce Perens K6BP.
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Journey: Service Account Lifecycle `[@ANCHOR: zero_sudo:COMM_journey_service_account_lifecycle]`

This journey tracks the lifecycle of a service account from its creation to its use in a secure execution context.

## 1. Provisioning
A module developer defines a service account in an XML data file.
```xml
<record id="user_my_daemon" model="res.users">
    <field name="name">My Daemon Service</field>
    <field name="login">my_daemon_service</field>
    <field name="is_service_account" eval="True"/> </record>
```
During creation, the system automatically assigns the account a cryptographically secure, 128-byte random password to ensure it cannot be accessed interactively `[@ANCHOR: zero_sudo:COMM_service_account_password_generation]`.

## 2. Verification of Isolation
The developer attempts to log in as `my_daemon_service` in the web browser. The `web_login` interceptor `[@ANCHOR: zero_sudo:COMM_web_login_interceptor]` detects the `is_service_account` flag and blocks access.

## 3. Secure Retrieval
In the module's Python code, the developer needs to perform an operation with elevated rights. They call `_get_service_uid` `[@ANCHOR: zero_sudo:COMM_get_service_uid]`.
This step is important for the lifecycle.


The system verifies that the account is indeed a service account and does not have dangerous global admin rights. It also utilizes lightweight Key-Value storage `[@ANCHOR: zero_sudo:COMM_set_kv_sql_check]` for internal state tracking during the retrieval lifecycle.

## 4. Execution
The developer uses the retrieved UID to create a new environment and execute the logic.
```python
svc_uid = utils._get_service_uid('my_module.user_my_daemon')
self.with_user(svc_uid).do_something_important()
```

## 5. Audit Trail
Any records created or modified will show "My Daemon Service" as the creator/modifier, providing a clear audit trail of which service account performed the action.

## 6. Scheduling a Cron Job for a Service Account
A service account with only narrow, deliberately-scoped access (e.g. read/create but not write, to
preserve tamper-resistance on an audit-style model) can still need to run a scheduled `ir.cron` job
against that same model. Odoo core's own `ir.actions.server._can_execute_action_on_records()` requires
**write** access to the cron's declared `model_id` before it will run the action at all -- a separate,
additional gate beyond `ir.model.access.csv`, checked regardless of what the action's own code actually
does. Setting `group_ids` on the `ir.cron` record (delegated from its underlying `ir.actions.server`)
authorizes the specific group for that specific action, satisfying this gate without broadening the
model's real ACL. `[@ANCHOR: zero_sudo:COMM_security_log_autovacuum_cron_runs]`
