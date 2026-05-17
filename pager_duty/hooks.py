# -*- coding: utf-8 -*-
import logging

_logger = logging.getLogger(__name__)

def post_init_hook(env):
    """
    Register daemon keys and trigger autodiscovery upon installation.
    """
    # Trigger autodiscovery if the system is completely empty
    if "pager.check" in env and not env["pager.check"].search_count([]):
        try:
            env["pager.check"]._run_autodiscovery()
        except Exception as e: # audit-ignore-catch-all
            _logger.warning("An error occurred during autodiscovery: %s", e)

    # Register Daemons for Automated Key Vault Provisioning
    if "daemon.key.registry" in env:
        try:
            env["daemon.key.registry"].register_daemon(
                daemon_name="Pager Duty - Generalized Monitor",
                user_xml_id="pager_duty.user_pager_service_internal",
                env_file_path="/var/lib/odoo/daemon_keys/pager_duty.env",
            )
        except Exception as e: # audit-ignore-catch-all
            _logger.warning("An error occurred during daemon registration: %s", e)
