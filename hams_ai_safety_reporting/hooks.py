# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Daemon key for the AI safety reporting service account (see README.md)."""

SERVICE_XMLID = "hams_ai_safety_reporting.user_ai_safety_reporting_service"
KEY_MANAGER_XMLID = "daemon_key_manager.user_daemon_key_manager_service"
KEY_FILE = "/opt/hams/etc/keys/ai_safety_reporting_service_internal.env"
KEY_NAME = "AI Safety Reporting"


# [@ANCHOR: hams_ai_safety_reporting:register_ai_safety_reporting_key]
# Verified by [@ANCHOR: hams_ai_safety_reporting:test_register_ai_safety_reporting_key]
def register_ai_safety_reporting_key(env):
    """Has the Daemon Key Manager issue the create-only AI safety reporting service account's API
    key and keep it in KEY_FILE, rotated like every daemon key. The Simulated Band bots and the
    Official Observer read it (``ODOO_KEY_FILE``) to file a safety ticket.

    Idempotent. A registry row that already holds KEY_FILE (the unit's key was first registered by
    hand on hams.com, before this module did it) keeps its name, because ``register_daemon`` finds a
    row by name and the file path is unique: registering under another name would be refused."""
    svc_uid = env["zero_sudo.security.utils"]._get_service_uid(SERVICE_XMLID)
    manager_env = env["zero_sudo.security.utils"]._get_service_env(KEY_MANAGER_XMLID)
    existing = manager_env["daemon.key.registry"].search([("env_file_path", "=", KEY_FILE)], limit=1)
    env["daemon.key.registry"].with_user(svc_uid).register_daemon(
        daemon_name=existing.name or KEY_NAME,
        user_xml_id=SERVICE_XMLID,
        env_file_path=KEY_FILE,
    )


# [@ANCHOR: hams_ai_safety_reporting:post_init_hook]
# Verified by [@ANCHOR: hams_ai_safety_reporting:test_register_ai_safety_reporting_key]
def post_init_hook(env):
    """Registers the key on a fresh install; ``migrations/1.1`` does it for installed databases."""
    register_ai_safety_reporting_key(env)
