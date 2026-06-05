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
    env_svc = env["zero_sudo.security.utils"]._get_service_env("compliance.user_compliance_service")

    _logger.info("Enforcing Cookie Consent Bar on all websites.")
    # Ensure we see all websites across all scopes.
    websites = env_svc["website"].with_context(active_test=False).search([], limit=1000)
    # AI Laziness Fix: Removed field check. If cookies_bar is missing, fail fast.
    # Some Odoo website logic expects singletons during write.
    for website in websites:
        website.write({"cookies_bar": True})

    # Non-Destructive Legal Page Mandate:
    # If a page already exists at these URLs, we unpublish our boilerplate to avoid duplication.
    # [@ANCHOR: compliance_website_aware_scope]
    # [@ANCHOR: story_automatic_legal_pages]
    # Verified by [@ANCHOR: test_compliance_non_destructive_mandate]
    legal_urls = ["/privacy", "/cookie-policy", "/terms", "/accessibility"]

    # ADR-0022: Prevent N+1 queries by pre-fetching outside the loop.
    # Use website_id=False in context to ensure we see pages across all websites.
    # Also use active_test=False to see unpublished pages.
    # We sort by id to ensure deterministic processing.
    all_pages = env_svc["website.page"].with_context(website_id=False, active_test=False).search([
        ("url", "in", legal_urls)
    ], order="id asc", limit=1000)

    def is_boilerplate(page):
        # AI Laziness Fix: Added guards to prevent AttributeErrors.
        # We expect boilerplate pages to have a view_id and a key.
        return page.view_id and page.view_id.key and page.view_id.key.startswith("compliance.compliance_")

    for bp in all_pages.filtered(is_boilerplate):
        # Identify custom pages in the SAME scope (same URL and same website_id).
        # We use IDs for comparison to be safe across environments.
        bp_website_id = bp.website_id.id if bp.website_id else False

        custom_pages = all_pages.filtered(lambda p: (
            p.url == bp.url and
            (p.website_id.id if p.website_id else False) == bp_website_id and
            p.id != bp.id and
            not is_boilerplate(p)
        ))

        if custom_pages:
            if bp.is_published:
                _logger.info("Shielding boilerplate page %s (url %s, scope %s) because custom version detected.",
                             bp.id, bp.url, bp_website_id or "global")
                bp.write({"is_published": False})
        else:
            # If no custom page exists in this specific scope, ensure our boilerplate is published.
            # This allows reverting to boilerplate by deleting the custom page and re-running the hook.
            if not bp.is_published:
                _logger.info("No custom page for %s in scope %s. Restoring boilerplate page %s.",
                             bp.url, bp_website_id or "global", bp.id)
                bp.write({"is_published": True})
