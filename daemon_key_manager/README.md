# Daemon Key Manager (`daemon_key_manager`)

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).*

The **Daemon Key Manager** is the centralized authority for managing Odoo API keys for external background programs (daemons). It generates secure API keys and saves them to local `.env` files, which the external programs can read. This removes the need for manual password management or insecure hardcoded credentials. It is a core part of the system's security, ensuring that background tasks have exactly the permissions they need and nothing more.

## 🚀 Quick Start: Integration API

Other modules should request daemon credentials during their installation (e.g., in a `post_init_hook`) or via a configuration wizard.

```python
def setup_daemon_credentials(env):
    # Idempotent registration and synchronous key generation
    # This call ensures the daemon is registered for 60-day rotations.
    env['daemon.key.registry'].register_daemon(
        daemon_name="My External Daemon",
        user_xml_id="my_module.my_service_account",
        env_file_path="/opt/hams/etc/keys/my_daemon.env"
    )
```

## 🛡️ Security Architecture

### Minimum Privilege Architecture
The module follows a strict "minimum privilege" policy. It uses a dedicated service account to perform its tasks, and every API key it generates belongs to a specific "Service Account" with limited rights.

**Security Principles:**
* **No Administrative Overreach:** Keys are generated specifically for the program that will use them.
* **Automatic Expiration:** Keys are set to expire in 90 days. The system rotates them every 60 days to ensure there is always a valid key.
* **Safety Fallback:** If a service account isn't configured for long-term keys, the system provides a 24-hour temporary key and logs a warning so an administrator can fix it.

### OS-Level Sandboxing
* **Strict Permissions:** `.env` files are created with `0600` (read/write only for the Odoo server process user).
* **Directory Isolation:** Parent directories are created with `0700` to prevent other users on the system from traversing into the key storage area. The key root directory (`/opt/hams/etc/keys`) itself is `0710`: the Odoo user has full access and its group may traverse and not list it, so a daemon account in that group can reach its own family's directory and cannot enumerate the others [@ANCHOR: COMM_key_root_directory_mode]. A family's key directory (`/opt/hams/etc/keys/<family>`, for the group `hamsd_<family>`) is owned by the Odoo user with that family's group at `0750`: Odoo writes, the one daemon account in the group traverses and reads, nobody else can enter [@ANCHOR: COMM_write_secure_env_file_group_directory].
* **Per-Daemon OS Group:** A registry may carry an `os_group` (`hamsd_<family>`, the OS group of the account the daemon runs as; `<family>` is the name suffix that identifies that account and group, and it also names the key directory `/opt/hams/etc/keys/<family>`). Its key file is then written `0640` with that group, so that account, and no other daemon, can read it. The Odoo user cannot `chown` to another user (it is unprivileged), only `chgrp` to a group it belongs to, so provisioning puts it in each such group; the file stays owned by Odoo, the only account that writes it, and so does the family's key directory (`/opt/hams/etc/keys/<family>`, group = the family, `0750`, re-asserted on every write, group first and mode second). A group key file may live only in its own family's directory; the registry constraint and the writer both refuse any other location [@ANCHOR: COMM_security_constraints_os_group_directory]. The file is `0600` until the group is set, so it is never group-readable under another group, and a failure (the group missing, or the server started before provisioning added it) leaves the previous key file untouched [@ANCHOR: COMM_write_secure_env_file_group]. Only a Daemon Key Manager may set the group, and the name must match `hamsd_<family>` so a registry cannot point a key at `hams_com` or any other privileged group [@ANCHOR: COMM_security_constraints_os_group]. See `docs/proposals/DAEMON_OS_ISOLATION_PLAN.md` in hams_com.
* **Path Validation:** All paths MUST start with `/opt/hams/etc/keys/`. The module strictly blocks directory traversal (`..`) and symlink attacks by resolving the `os.path.realpath` of the requested path before performing any file operations [@ANCHOR: COMM_security_constraints_path].

To further protect the integrity of the host system, additional safety mechanisms are enforced across the platform.

* **System Directory Protection:** Writing to sensitive system directories is explicitly forbidden regardless of the prefix check [@ANCHOR: COMM_write_secure_env_file_logic].

### Automated Key Rotation
Keys are automatically rotated every 60 days via an `ir.cron` job [@ANCHOR: COMM_cron_rotation_trigger].

* **Graceful Failure:** Stateless batching (processing 10 records at a time and re-triggering) ensures that one failed file-write or database error does not block other rotations. Failures are logged, and the system attempts to continue.
* **Buffer Period:** New keys are generated with a 90-day expiration, providing a 30-day "grace period" for the 60-day rotation cycle to succeed.
* **Self-Healing Daemons:** Daemons utilizing these keys MUST be designed to catch `AccessError` responses from Odoo, re-read their assigned `.env` file from the disk, and retry the request.

---

## 🛠️ Technical Reference

### 1. Storage & Orchestration Mandate
All credentials **MUST** be written to `/opt/hams/etc/keys/`.
In containerized/orchestrated environments:
* **Odoo Container:** Mount the volume as **Read/Write**.
* **Daemon Containers:** Mount the volume as **Read-Only**.

### 2. Core API Methods (Public)

#### `register_daemon(daemon_name, user_xml_id, env_file_path)`
* **`daemon_name`**: A unique string identifier for the external service.
* **`user_xml_id`**: The XML ID of the service account record (e.g., `pager_duty.user_pager_service_internal`). Must have `is_service_account = True`.
* **`env_file_path`**: Absolute path where the `.env` file should be written. Must reside within `/opt/hams/etc/keys/`.
* **Behavior**: Idempotent. Immediately triggers key generation and writes the file. Associates registry with the service account's company.

#### `action_rotate_key()`
* **Use Case**: Manually rotate the key for a specific daemon via UI or code.
* **Behavior**: Revokes existing key and generates a new one synchronously.
* **Security**: Only accessible to members of `Daemon Key Management / Manager`.

#### `rotate_own_key(daemon_name, current_key)` (remote self-rotation)
* **Use Case**: A daemon that runs on another machine than Odoo, whose key file Odoo cannot write
  (e.g. a sync daemon moved to a separate machine for a non-datacenter egress path). A manager sets **Remote Self-Rotation** on its registry; the
  cron, `action_force_provision_all()` and `action_rotate_key()` then leave that registry alone.
* **Behavior**: Called over JSON-2 by the daemon itself, authenticated with its current key, which it
  also passes as `current_key`. Two phases: with the active key, once the key is more than 59 days old,
  it returns `{"status": "issued", "login", "key"}` and revokes nothing; with that new key it returns
  `{"status": "confirmed"}`, revokes the old key and writes the new one to `env_file_path`. Otherwise
  `{"status": "not_due", "next_rotation"}`. The daemon calls it at the start of each run and writes the new key to its own key file
  atomically between the two calls.
* **Security**: Only the registry's own service account, presenting a live key of that registry.

#### `action_force_provision_all()`
* **Use Case**: Used during system bootstrapping (e.g., via systemd or Kubernetes init containers) or emergency rotations.
* **Shell Invocation**:
  ```bash
  odoo-bin shell -d hams --no-http -e "env['daemon.key.registry'].action_force_provision_all(); env.cr.commit()"
  ```

### 3. Core Internal Methods (For Developers & AIs)

#### `_rotate_key_and_write_file(pre_fetched_keys=None)`
* **Behavior**: The underlying mechanism that revokes old keys via `res.users.apikeys`, generates a new 90-day key, and calls `_write_secure_env_file`. Handles validation of `__system__` restrictions.

#### `_write_secure_env_file(path, login, key, group=None)`
* **Behavior**: Safely writes the `.env` file. Enforces `0600` on the file (or `0640` with the given `hamsd_<family>` group) and `0700` on parent directories (`0710` on the key root, `0750` with the family group on a family's key directory). Prevents path traversal.

#### `_cron_rotate_all_keys()`
* **Behavior**: Triggered by cron. Uses a batch limit of 10 and triggers itself recursively to avoid transaction timeouts. Commits successful writes immediately and rolls back individual failures.

### 4. File Format (.env)
```env
# Auto-generated by daemon.key.registry
ODOO_RPC_LOGIN=service_account_login
ODOO_RPC_KEY=12345abcd...
```

---

## 📖 Stories & Journeys

* [Registering a New External Daemon](docs/stories/daemon_registration.md)
* [Manual Force Provisioning](docs/stories/force_provisioning.md)
* [Automated 60-Day Key Rotation](docs/stories/key_rotation.md)
* [Lifecycle of a Daemon API Key](docs/journeys/api_key_lifecycle.md)
* [Zero-Sudo Refactoring Story](docs/stories/zero_sudo_refactoring.md)

---

## 🧪 Verification
* **register_daemon_api**: The registration flow ensures that all daemon keys are properly provisioned for correct environments, verified by [@ANCHOR: COMM_test_register_daemon_api].

* **force_provision_all**: Bootstrapping functionality is thoroughly tested to guarantee rapid restoration in an emergency, verified by [@ANCHOR: COMM_test_force_provisioning].

* **security_constraints**: To prevent malicious behavior, strict sandboxing and directory path validation are verified by [@ANCHOR: COMM_test_security_constraints].

* **ui_tour**: Interactive interface flows inside Odoo for key management are comprehensively verified by [@ANCHOR: COMM_test_daemon_key_manager_tour].

* **unauthorized_access**: Zero-sudo barriers blocking non-system service accounts from escalating permissions are verified by [@ANCHOR: COMM_test_unauthorized_access].

* **key_ownership**: Finally, proper multi-tenant key isolation is checked and verified by [@ANCHOR: COMM_test_key_ownership].
