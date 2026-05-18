# -*- coding: utf-8 -*-


def post_init_hook(env):
    """
    Register daemon keys upon installation.
    """
    # Register Backup Worker for Automated Key Vault Provisioning
    env_svc = env["zero_sudo.security.utils"]._get_service_env(
        "zero_sudo.odoo_facility_service_internal"
    )

    if "daemon.key.registry" in env_svc:
        env_svc["daemon.key.registry"].register_daemon(
            daemon_name="Backup Worker RabbitMQ Consumer",
            user_xml_id="backup_management.user_backup_service_internal",
            env_file_path="/var/lib/odoo/daemon_keys/backup_worker.env",
        )
