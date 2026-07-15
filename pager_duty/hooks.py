# SPDX-License-Identifier: AGPL-3.0-or-later
# This software is distributed under the terms of the Affero General Public License (AGPL-3).

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
    if "pager.check" in env and not env["pager.check"].search([], limit=1):
        try:
            env["pager.check"]._run_autodiscovery()
        except Exception:  # audit-ignore-catch-all
            _logger.exception("An error occurred during autodiscovery:")

    # Register Daemons for Automated Key Vault Provisioning
    if "daemon.key.registry" in env:
        env["daemon.key.registry"].with_user(env.ref("base.user_admin")).register_daemon(
            daemon_name="Pager Duty - Generalized Monitor",
            user_xml_id="pager_duty.user_pager_service_internal",
            env_file_path="/opt/hams/etc/keys/pager_duty.env",
        )
