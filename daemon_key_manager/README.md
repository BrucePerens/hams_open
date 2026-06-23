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
        env_file_path="/var/lib/odoo/daemon_keys/my_daemon.env"
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
* **Path Validation:** All paths MUST start with `/var/lib/odoo/daemon_keys/`. The module strictly blocks directory traversal (`..`) and symlink attacks by resolving the `os.path.realpath` of the requested path before performing any file operations [@ANCHOR: security_constraints_path].
* **System Directory Protection:** Writing to sensitive system directories (like `/etc`, `/root`, `/boot`, `/home`, `/usr`, `/bin`, `/lib`, `/var/log`) is explicitly forbidden regardless of the prefix check [@ANCHOR: write_secure_env_file_logic].

### Automated Key Rotation
Keys are automatically rotated every 60 days via an `ir.cron` job [@ANCHOR: cron_rotation_trigger].
* **Graceful Failure:** Stateless batching (processing 10 records at a time and re-triggering) ensures that one failed file-write or database error does not block other rotations. Failures are logged, and the system attempts to continue with the next daemon [@ANCHOR: cron_rotation_logic].
* **Buffer Period:** New keys are generated with a 90-day expiration, providing a 30-day "grace period" for the 60-day rotation cycle to succeed in case of transient server issues.
* **Self-Healing Daemons:** Daemons utilizing these keys MUST be designed to catch `AccessError` responses from Odoo, re-read their assigned `.env` file from the disk, and retry the request. This ensures continuous operation across key rotations [@ANCHOR: daemon_self_healing].

---

## 🛠️ Technical Reference

### 1. Storage & Orchestration Mandate
All credentials **MUST** be written to `/var/lib/odoo/daemon_keys/`.
In containerized/orchestrated environments:
* **Odoo Container:** Mount the volume as **Read/Write**.
* **Daemon Containers:** Mount the volume as **Read-Only**.

### 2. Core API Methods

#### `register_daemon(daemon_name, user_xml_id, env_file_path)` [@ANCHOR: register_daemon_api]
* **`daemon_name`**: A unique string identifier for the external service.
* **`user_xml_id`**: The XML ID of the service account record (e.g., `pager_duty.user_pager_service_internal`). This account must have `is_service_account` set to `True`.
* **`env_file_path`**: The absolute path where the `.env` file should be written. It must reside within `/var/lib/odoo/daemon_keys/`.
* **Behavior**: This method is idempotent. If a daemon with the same name exists, its service account and path are updated. It immediately triggers the generation of the first API key and writes the file. It also ensures the registry is associated with the service account's company [@ANCHOR: register_daemon_logic] [@ANCHOR: register_daemon_idempotency].

#### `action_rotate_key()` [@ANCHOR: action_rotate_key_api]
* **Use Case**: Manually rotate the key for a specific daemon. This is the preferred method for rotating individual credentials if they are suspected of being compromised.
* **Behavior**: Revokes the existing key and generates a new one synchronously.
* **Security**: Only accessible to members of the `Daemon Key Management / Manager` group.

#### `action_force_provision_all()` [@ANCHOR: action_force_provision_all_api]
* **Use Case**: Used during system bootstrapping (e.g., via systemd or Kubernetes init containers) to ensure all keys are present on disk before daemons start. Also used for emergency rotation of all keys.
* **Shell Invocation**:
  ```bash
  odoo-bin shell -d hams --no-http -e "env['daemon.key.registry'].action_force_provision_all(); env.cr.commit()"
  ```
* **Security**: Only accessible to users in the `Daemon Key Management / Manager` group. Internally, it elevates to the service account to perform the privileged key generation [@ANCHOR: force_provision_logic].

### 3. File Format (.env) [@ANCHOR: write_secure_env_file_logic]
```env
# Auto-generated by daemon.key.registry
ODOO_RPC_LOGIN=service_account_login
ODOO_RPC_KEY=12345abcd...
```

---

## 📖 Stories & Journeys

* [Registering a New External Daemon](hams_shared/docs/stories/daemon_registration.md)
* [Manual Force Provisioning](hams_shared/docs/stories/force_provisioning.md)
* [Automated 60-Day Key Rotation](hams_shared/docs/stories/key_rotation.md)
* [Lifecycle of a Daemon API Key](hams_shared/docs/journeys/api_key_lifecycle.md)
* [Bootstrapping a Containerized Environment](hams_shared/docs/journeys/container_bootstrapping.md)
* [Zero-Sudo Refactoring Story](hams_shared/docs/stories/zero_sudo_refactoring.md)

---

## 🧪 Verification
* **register_daemon_api**: Verified by [@ANCHOR: test_register_daemon_api]
* **force_provision_all**: Verified by [@ANCHOR: test_force_provisioning]
* **security_constraints**: Verified by [@ANCHOR: test_security_constraints]
* **ui_tour**: Verified by [@ANCHOR: test_daemon_key_manager_tour]
* **unauthorized_access**: Verified by [@ANCHOR: test_unauthorized_access]
* **key_ownership**: Verified by [@ANCHOR: test_key_ownership]
* **documentation_installed**: Verified by [@ANCHOR: documentation_installed] [@ANCHOR: test_documentation_installed]
