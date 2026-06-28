# -*- coding: utf-8 -*-
import logging

_logger = logging.getLogger(__name__)


def post_init_hook(env):
    """
    Register daemon keys and trigger autodiscovery upon installation.
    """
    # The _bootstrap_knowledge_docs function handles document installation;
    # do not create redundant post-init hooks.
    # We keep the autodiscovery logic as it's not handled by bootstrap.

    # Trigger autodiscovery if the system is completely empty
    if "pager.check" in env and not env["pager.check"].search_count([]):
        try:
            env["pager.check"]._run_autodiscovery()
        except Exception as e:  # audit-ignore-catch-all
            _logger.warning("An error occurred during autodiscovery: %s", e)

    # Register Daemons for Automated Key Vault Provisioning
    if "daemon.key.registry" in env:
        env["daemon.key.registry"].register_daemon(
            daemon_name="Pager Duty - Generalized Monitor",
            user_xml_id="pager_duty.user_pager_service_internal",
            env_file_path="/var/lib/odoo/daemon_keys/pager_duty.env",
        )
