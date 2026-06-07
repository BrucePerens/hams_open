# -*- coding: utf-8 -*-
import logging

_logger = logging.getLogger(__name__)


def post_init_hook(env):
    """
    Executes automatically upon module installation.
    Analyzes the Cloudflare perimeter and syncs or deploys the configuration natively.
    """
    _logger.info("Initializing Cloudflare Edge Orchestration...")

    # Execute Zero-Sudo invocation of the config manager
    # ADR-0055: Use service accounts for initialization
    utils = env["zero_sudo.security.utils"]
    svc_uid = utils._get_service_uid("cloudflare.user_cloudflare_waf")
    env["cloudflare.config.manager"].with_user(svc_uid).initialize_cloudflare_state()
