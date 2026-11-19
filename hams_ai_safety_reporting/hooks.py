# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Daemon key for the AI safety reporting service account (see README.md)."""
import grp
import logging

_logger = logging.getLogger(__name__)

SERVICE_XMLID = "hams_ai_safety_reporting.user_ai_safety_reporting_service"
KEY_MANAGER_XMLID = "daemon_key_manager.user_daemon_key_manager_service"
# The simulated bots and the Official Observer share one low-privilege account, hamsd_simbots (Bruce, 2026-11-19): the key lives in
# its family key directory and is written 0640 with that group (daemon_key_manager). Until 1.2 it was a flat file, odoo:odoo 0600.
OS_GROUP = "hamsd_simbots"
KEY_FILE = "/opt/hams/etc/keys/simbots/ai_safety_reporting_service_internal.env"
OLD_KEY_FILE = "/opt/hams/etc/keys/ai_safety_reporting_service_internal.env"
KEY_NAME = "AI Safety Reporting"


# [@ANCHOR: hams_ai_safety_reporting:register_ai_safety_reporting_key]
# Verified by [@ANCHOR: hams_ai_safety_reporting:test_register_ai_safety_reporting_key]
def register_ai_safety_reporting_key(env):
    """Has the Daemon Key Manager issue the create-only AI safety reporting service account's API
    key and keep it in KEY_FILE, rotated like every daemon key. The Simulated Band bots and the
    Official Observer read it (``ODOO_KEY_FILE``) to file a safety ticket.

    Idempotent. A registry row that already holds KEY_FILE or the flat OLD_KEY_FILE (the unit's key was first registered by
    hand on hams.com, before this module did it) keeps its name, because ``register_daemon`` finds a row by name and the file path
    is unique: registering under another name would be refused. The row is moved to KEY_FILE and given OS_GROUP.

    Returns False and registers nothing while the host has no OS_GROUP: the key manager refuses to write a group's key file before
    provisioning created the account, and a half-moved key would revoke the working one. Provision the account first."""
    if OS_GROUP not in {entry.gr_name for entry in grp.getgrall()}:
        _logger.warning(
            "hams_ai_safety_reporting: OS group %s does not exist on this host, so the safety reporting key was NOT registered or moved. "
            "Provision the hamsd_simbots account first (provision.py), then upgrade this module.",
            OS_GROUP,
        )
        return False
    manager_env = env["zero_sudo.security.utils"]._get_service_env(KEY_MANAGER_XMLID)
    existing = manager_env["daemon.key.registry"].search([("env_file_path", "in", (KEY_FILE, OLD_KEY_FILE))], limit=1)
    # A manager (not the service account itself) registers it: only a manager may set the OS group.
    manager_env["daemon.key.registry"].register_daemon(
        daemon_name=existing.name or KEY_NAME,
        user_xml_id=SERVICE_XMLID,
        env_file_path=KEY_FILE,
        os_group=OS_GROUP,
    )
    return True


# [@ANCHOR: hams_ai_safety_reporting:post_init_hook]
# Verified by [@ANCHOR: hams_ai_safety_reporting:test_register_ai_safety_reporting_key]
def post_init_hook(env):
    """Registers the key on a fresh install; ``migrations/1.1`` does it for installed databases."""
    register_ai_safety_reporting_key(env)
