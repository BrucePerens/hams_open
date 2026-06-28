# -*- coding: utf-8 -*-
import logging
from . import models
from . import controllers

_logger = logging.getLogger(__name__)


def post_init_hook(env):
    # The _bootstrap_knowledge_docs function handles document installation;
    # do not create redundant post-init hooks.
    # We keep the autodiscovery logic as it's not handled by bootstrap.

    # Trigger autodiscovery if the system is completely empty
    if "pager.check" in env and not env["pager.check"].search_count([]):
        try:
            env["pager.check"]._run_autodiscovery()
        except Exception as e:  # audit-ignore-catch-all
            _logger.warning("An error occurred during autodiscovery: %s", e)
