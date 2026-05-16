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
    env_svc = env["zero_sudo.security.utils"]._get_service_env("compliance.user_compliance_service")

    _logger.info("Enforcing Cookie Consent Bar on all websites.")
    websites = env_svc["website"].search([], limit=10000)
    if "cookies_bar" in env_svc["website"]._fields:
        websites.write({"cookies_bar": True})

    # Non-Destructive Legal Page Mandate:
    # If a page already exists at these URLs, we unpublish our boilerplate to avoid duplication.
    # ADR-0022: Prevent N+1 queries by pre-fetching outside the loop.
    # [@ANCHOR: story_automatic_legal_pages]
    # Verified by [@ANCHOR: test_compliance_non_destructive_mandate]
    legal_urls = ["/privacy", "/cookie-policy", "/terms"]

    # Pre-fetch all pages for the target URLs
    all_pages = env_svc["website.page"].search([
        ("url", "in", legal_urls)
    ], limit=1000)

    for url in legal_urls:
        url_pages = all_pages.filtered(lambda p: p.url == url)

        # Check for pages that are NOT owned by this module
        custom_pages = url_pages.filtered(lambda p: not p.view_id.key.startswith("compliance.compliance_"))

        if custom_pages:
            _logger.info("Custom page detected at %s. Shielding existing content.", url)
            # Find OUR boilerplate pages and unpublish them to defer to the site owner's version
            boilerplate_pages = url_pages.filtered(lambda p: p.view_id.key.startswith("compliance.compliance_"))
            if boilerplate_pages:
                boilerplate_pages.write({"is_published": False})
