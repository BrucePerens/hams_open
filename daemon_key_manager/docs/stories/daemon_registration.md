# Story: Registering a New External Daemon

As a **Developer** of an external Odoo integration (like a monitoring service or a backup manager),
I want to **register my daemon** with the Odoo instance,
so that it receives a secure, auto-rotating API key without me having to manage credentials manually.

## Scenario: Initial Module Installation

1.  The developer creates a new Odoo module for their daemon.
2.  In the module's `post_init_hook` or a configuration wizard, they call the `register_daemon` API [@ANCHOR: COMM_register_daemon_api].

3.  The `daemon_key_manager` verifies the service account and the requested file path [@ANCHOR: COMM_register_daemon_logic].
4.  A new entry is created in the `daemon.key.registry`.
5.  An initial API key is generated and securely written to the specified `.env` file on disk [@ANCHOR: COMM_write_secure_env_file_logic].
6.  The daemon can now boot and read its credentials from `/opt/hams/etc/keys/my_daemon.env`.

## Key Logic
- The registration is idempotent; calling it again updates the existing registration [@ANCHOR: COMM_register_daemon_idempotency].

- Security constraints ensure that only service accounts can be used [@ANCHOR: COMM_security_constraints_user] and files are written to allowed paths [@ANCHOR: COMM_security_constraints_path].

- Automated privilege assignment ensures the daemon has the correct permissions for long-lived keys [@ANCHOR: COMM_privilege_escalation_bypass].

## Scenario: A daemon that runs under its own OS account

1.  The daemon's unit runs as a dedicated account (`hamsd_<family>`), not as the Odoo user.
2.  A Daemon Key Manager registers it with that group (`register_daemon(..., os_group="hamsd_<family>")`) [@ANCHOR: COMM_register_daemon_os_group_manager_only].
3.  The key file is written `0640` with that group into the family's own key directory (`/opt/hams/etc/keys/<family>`, owned by Odoo, that group, `0750`), so only that account reads it [@ANCHOR: COMM_write_secure_env_file_group] [@ANCHOR: COMM_write_secure_env_file_group_directory].
4.  Every other daemon, which is in no such group, gets "permission denied" opening it.
