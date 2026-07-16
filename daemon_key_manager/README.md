# Daemon Key Manager (`daemon_key_manager`)

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).*

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
* **Directory Isolation:** Parent directories are created with `0700` to prevent other users on the system from traversing into the key storage area.
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

#### `action_force_provision_all()`
* **Use Case**: Used during system bootstrapping (e.g., via systemd or Kubernetes init containers) or emergency rotations.
* **Shell Invocation**:
  ```bash
  odoo-bin shell -d hams --no-http -e "env['daemon.key.registry'].action_force_provision_all(); env.cr.commit()"
  ```

### 3. Core Internal Methods (For Developers & AIs)

#### `_rotate_key_and_write_file(pre_fetched_keys=None)`
* **Behavior**: The underlying mechanism that revokes old keys via `res.users.apikeys`, generates a new 90-day key, and calls `_write_secure_env_file`. Handles validation of `__system__` restrictions.

#### `_write_secure_env_file(path, login, key)`
* **Behavior**: Safely writes the `.env` file. Enforces `0600` on the file and `0700` on parent directories. Prevents path traversal.

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
* [Bootstrapping a Containerized Environment](docs/journeys/container_bootstrapping.md)
* [Zero-Sudo Refactoring Story](docs/stories/zero_sudo_refactoring.md)

---

## 🧪 Verification
* **register_daemon_api**: The registration flow ensures that all daemon keys are properly provisioned for correct environments, verified by [@ANCHOR: COMM_test_register_daemon_api].

* **force_provision_all**: Bootstrapping functionality is thoroughly tested to guarantee rapid restoration in an emergency, verified by [@ANCHOR: COMM_test_force_provisioning].

* **security_constraints**: To prevent malicious behavior, strict sandboxing and directory path validation are verified by [@ANCHOR: COMM_test_security_constraints].

* **ui_tour**: Interactive interface flows inside Odoo for key management are comprehensively verified by [@ANCHOR: COMM_test_daemon_key_manager_tour].

* **unauthorized_access**: Zero-sudo barriers blocking non-system service accounts from escalating permissions are verified by [@ANCHOR: COMM_test_unauthorized_access].

* **key_ownership**: Finally, proper multi-tenant key isolation is checked and verified by [@ANCHOR: COMM_test_key_ownership].
