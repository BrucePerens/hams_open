# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# This software is released under the AGPL-3.0 License.


def post_init_hook(env):
    """
    Register daemon keys upon installation.
    """
    # Register Backup Worker for Automated Key Vault Provisioning
    svc_uid = env["zero_sudo.security.utils"]._get_service_uid(
        "backup_management.user_backup_service_internal"
    )

    env["daemon.key.registry"].with_user(svc_uid).register_daemon(
        daemon_name="Backup Worker RabbitMQ Consumer",
        user_xml_id="backup_management.user_backup_service_internal",
        env_file_path="/opt/hams/etc/keys/backup_worker.env",
    )

    env["ir.module.module"].with_user(svc_uid)._bootstrap_knowledge_docs()
