# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. AGPL-3.0.
import logging

_logger = logging.getLogger(__name__)


def post_init_hook(env):
    """
    Register daemon keys upon installation.
    """
    if "daemon.key.registry" in env:
        env["daemon.key.registry"].register_daemon(
            daemon_name="Redis Cache Manager",
            user_xml_id="distributed_redis_cache.cache_manager_service_internal",
            env_file_path="/var/lib/odoo/daemon_keys/cache_manager.env",
        )
        _logger.info("Registered Redis Cache Manager daemon keys.")

    # Synchronize Redis configuration to the daemon env file via ir.config_parameter.
    # The daemon_key_manager does not automatically include config_parameters,
    # so we must handle it or wait for a refresh cycle.
    # For now, we rely on the daemon using its fallback or being restarted
    # after manual configuration.
