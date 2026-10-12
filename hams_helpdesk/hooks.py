# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Daemon key for the exception reporter (see daemon/README.md)."""

SERVICE_XMLID = "hams_helpdesk.user_exception_reporter_service"
KEY_FILE = "/opt/hams/etc/keys/exception_reporter.env"


# [@ANCHOR: hams_helpdesk:register_exception_reporter_key]
def register_exception_reporter_key(env):
    """Has the Daemon Key Manager issue the exception reporter service account's API key and keep it
    in KEY_FILE (rotated like every daemon key). The key lets the OnFailure reporter and a daemon's
    ``reporting_failures`` call ``report_daemon_failure``, and that account can do nothing else.
    Idempotent: register_daemon updates an existing registry."""
    svc_uid = env["zero_sudo.security.utils"]._get_service_uid(SERVICE_XMLID)
    env["daemon.key.registry"].with_user(svc_uid).register_daemon(
        daemon_name="Exception Reporter (OnFailure and daemon helper)",
        user_xml_id=SERVICE_XMLID,
        env_file_path=KEY_FILE,
    )


def post_init_hook(env):
    """Registers the key on a fresh install; ``migrations/1.1`` does it for installed databases."""
    register_exception_reporter_key(env)
