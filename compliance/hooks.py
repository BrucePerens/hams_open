# -*- coding: utf-8 -*-
# Copyright (c) 2024 Bruce Perens K6BP
# SPDX-License-Identifier: AGPL-3.0-only
import logging

_logger = logging.getLogger(__name__)


def post_init_hook(env):
    """
    Hook executed upon module installation.
    1. Enforces the use of Odoo's native cookie consent banner.
    2. Ensures legal pages are non-destructively provisioned.
    """
    # [@ANCHOR: COMM_journey_compliance_setup]

    # # Verified by [@ANCHOR: COMM_test_compliance_ui_tour]

    # [@ANCHOR: COMM_compliance_post_init_cookie_bar]

    # [@ANCHOR: COMM_story_cookie_consent]

    # # Verified by [@ANCHOR: COMM_test_compliance_post_init_cookie_bar]

    # # Verified by [@ANCHOR: COMM_test_compliance_ui_tour]

    # ADR-0002: Zero-Sudo Architecture. We must not use .sudo()
    # or stay as SUPERUSER.
    # We switch to a dedicated micro-privilege service account.
    # [@ANCHOR: COMM_compliance_zero_sudo_impersonation]
    svc_uid = env["zero_sudo.security.utils"]._get_service_uid(
        "compliance.user_compliance_service"
    )
    env_svc = env(user=svc_uid)

    # Execute DDL directly instead of using ir.actions.server
    sql = """
    CREATE OR REPLACE FUNCTION compliance_enforce_protection() RETURNS VOID AS $$
    BEGIN
        -- 1. Enforce Cookie Bar on all websites
        UPDATE website SET cookies_bar = TRUE;

        -- 2. Shield boilerplate pages where custom versions exist
        -- We identify boilerplate by the 'compliance.compliance_' key prefix
        UPDATE website_page bp
        SET is_published = FALSE
        FROM ir_ui_view v
        WHERE bp.view_id = v.id
          AND v.key LIKE 'compliance.compliance_%'
          AND bp.is_published = TRUE
          AND EXISTS (
              SELECT 1 FROM website_page cp
              JOIN ir_ui_view cv ON cp.view_id = cv.id
              WHERE cp.url = bp.url
                AND cp.website_id IS NOT DISTINCT FROM bp.website_id
                AND cp.id != bp.id
                AND cv.key NOT LIKE 'compliance.compliance_%'
          );

        -- 3. Restore boilerplate pages where no custom versions exist
        UPDATE website_page bp
        SET is_published = TRUE
        FROM ir_ui_view v
        WHERE bp.view_id = v.id
          AND v.key LIKE 'compliance.compliance_%'
          AND bp.is_published = FALSE
          AND NOT EXISTS (
              SELECT 1 FROM website_page cp
              JOIN ir_ui_view cv ON cp.view_id = cv.id
              WHERE cp.url = bp.url
                AND cp.website_id IS NOT DISTINCT FROM bp.website_id
                AND cp.id != bp.id
                AND cv.key NOT LIKE 'compliance.compliance_%'
          );
    END;
    $$ LANGUAGE plpgsql;
    """
    env_svc.flush_all()
    try:
        with env_svc.cr.savepoint():
            env_svc.cr.execute(sql)
    except Exception as e: # audit-ignore-catch-all
        _logger.error("Failed to execute compliance_enforce_protection DDL: %s", e)

    _logger.info("Executing Compliance Enforcement via Postgres Procedure.")
    # Performance Optimization: Reduced dozens of ORM round-trips
    # to a single Postgres procedure call.
    # # Verified by [@ANCHOR: COMM_test_compliance_postgres_procedures]
    env_svc.flush_all()
    try:
        with env_svc.cr.savepoint():
            env_svc.cr.execute("SELECT compliance_enforce_protection()")
    except Exception as e: # audit-ignore-catch-all
        _logger.error("Failed to call compliance_enforce_protection(): %s", e)

    env_svc["ir.module.module"]._bootstrap_knowledge_docs()

    # We must invalidate the ORM cache because the Postgres procedure modified
    # records directly.
    env_svc["website"].invalidate_model(["cookies_bar"])
    env_svc["website.page"].invalidate_model(["is_published"])
