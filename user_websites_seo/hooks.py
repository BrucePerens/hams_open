# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
import logging

_logger = logging.getLogger(__name__)


def post_init_hook(env):
    # [@ANCHOR: COMM_soft_dependency_docs_installation]

    # # Verified by [@ANCHOR: COMM_test_soft_dependency_docs_installation]
    utils = env["zero_sudo.security.utils"]
    try:
        if not utils._get_system_param("user_websites_seo.docs_installed"):
            # Ensure the service account exists before signaling
            utils._get_service_uid("user_websites.user_websites_service_account")
            # Signal completion or perform SEO-specific bootstrap tasks.
            utils._set_system_param("user_websites_seo.docs_installed", "True")
    except Exception as e:  # audit-ignore-catch-all  # fmt: skip
        _logger.warning("SEO Docs installation signal failed: %s", e)
