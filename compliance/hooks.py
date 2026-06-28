# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
import logging

_logger = logging.getLogger(__name__)


def post_init_hook(env):
    """
    Hook executed upon module installation.
    1. Enforces the use of Odoo's native cookie consent banner.
    2. Ensures legal pages are non-destructively provisioned.
    """
    # [@ANCHOR: journey_compliance_setup]
    # Verified by [@ANCHOR: test_compliance_ui_tour]
    # [@ANCHOR: compliance_post_init_cookie_bar]
    # [@ANCHOR: story_cookie_consent]
    # Verified by [@ANCHOR: test_compliance_post_init_cookie_bar]
    # Verified by [@ANCHOR: test_compliance_ui_tour]

    # ADR-0002: Zero-Sudo Architecture. We must not use .sudo() or stay as SUPERUSER.
    # We switch to a dedicated micro-privilege service account.
    # [@ANCHOR: compliance_zero_sudo_impersonation]
    env_svc = env["zero_sudo.security.utils"]._get_service_env(
        "compliance.user_compliance_service"
    )

    _logger.info("Executing Compliance Enforcement via Postgres Procedure.")
    # Performance Optimization: Reduced dozens of ORM round-trips to a single Postgres procedure call.
    # Verified by [@ANCHOR: test_compliance_postgres_procedures]
    env_svc.cr.execute("SELECT compliance_enforce_protection()")

    # We must invalidate the ORM cache because the Postgres procedure modified records directly.
    env_svc["website"].invalidate_model(["cookies_bar"])
    env_svc["website.page"].invalidate_model(["is_published"])
