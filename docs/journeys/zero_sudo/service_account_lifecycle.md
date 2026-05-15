# Journey: Service Account Lifecycle `[@ANCHOR: journey_service_account_lifecycle]`

This journey tracks the lifecycle of a service account from its creation to its use in a secure execution context.

## 1. Provisioning
A module developer defines a service account in an XML data file.
```xml
<record id="user_my_daemon" model="res.users">
    <field name="name">My Daemon Service</field>
    <field name="login">my_daemon_service</field>
    <field name="is_service_account" eval="True"/> <!-- [@ANCHOR: is_service_account_field] -->
</record>
```

## 2. Verification of Isolation
The developer attempts to log in as `my_daemon_service` in the web browser. The `web_login` interceptor `[@ANCHOR: web_login_interceptor]` detects the `is_service_account` flag and blocks access.

## 3. Secure Retrieval
In the module's Python code, the developer needs to perform an operation with elevated rights. They call `_get_service_uid` `[@ANCHOR: get_service_uid]`.
The system verifies that the account is indeed a service account and does not have dangerous global admin rights.

## 4. Execution
The developer uses the retrieved UID to create a new environment and execute the logic.
```python
svc_uid = utils._get_service_uid('my_module.user_my_daemon')
self.with_user(svc_uid).do_something_important()
```

## 5. Audit Trail
Any records created or modified will show "My Daemon Service" as the creator/modifier, providing a clear audit trail of which service account performed the action.
