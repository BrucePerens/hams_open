# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Daemon key for the exception reporter (see daemon/README.md)."""
import grp

SERVICE_XMLID = "hams_helpdesk.user_exception_reporter_service"
# The reporter runs as its own account, hamsd_exception_report (hams_shared MANIFEST): its key is in that family's key directory with
# the account's group. A host without the account (a developer box that was not provisioned) keeps the flat, odoo-only file.
OS_GROUP = "hamsd_exception_report"
KEY_FILE = "/opt/hams/etc/keys/exception_report/exception_reporter.env"
LEGACY_KEY_FILE = "/opt/hams/etc/keys/exception_reporter.env"


# [@ANCHOR: hams_helpdesk:exception_reporter_key_target]
# Verified by [@ANCHOR: hams_helpdesk:test_exception_reporter_key_target]
def exception_reporter_key_target():
    """(key file, OS group) the reporter key is registered with: the family directory and group where the account exists on this
    host, else the legacy flat file with no group (a rotation could not write the family directory before provisioning)."""
    if OS_GROUP not in {entry.gr_name for entry in grp.getgrall()}:
        return LEGACY_KEY_FILE, None
    return KEY_FILE, OS_GROUP


# [@ANCHOR: hams_helpdesk:register_exception_reporter_key]
# Verified by [@ANCHOR: hams_helpdesk:test_register_exception_reporter_key]
def register_exception_reporter_key(env):
    """Has the Daemon Key Manager issue the exception reporter service account's API key and keep it
    in KEY_FILE (rotated like every daemon key). The key lets the OnFailure reporter and a daemon's
    ``reporting_failures`` call ``report_daemon_failure``, and that account can do nothing else.
    Idempotent: register_daemon updates an existing registry."""
    utils = env["zero_sudo.security.utils"]
    path, group = exception_reporter_key_target()
    # Setting an OS group is the Daemon Key Manager's to do (a service account may not widen who reads its own key file).
    caller_uid = utils._get_service_uid("daemon_key_manager.user_daemon_key_manager_service" if group else SERVICE_XMLID)
    env["daemon.key.registry"].with_user(caller_uid).register_daemon(
        daemon_name="Exception Reporter (OnFailure and daemon helper)",
        user_xml_id=SERVICE_XMLID,
        env_file_path=path,
        os_group=group,
    )


# [@ANCHOR: hams_helpdesk:post_init_hook]
def post_init_hook(env):
    """Registers the key on a fresh install; ``migrations/1.1`` does it for installed databases."""
    register_exception_reporter_key(env)
