# SPDX-License-Identifier: AGPL-3.0-or-later
# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
import logging

_logger = logging.getLogger(__name__)

_PAGER_BOARD_SQL = """\
CREATE OR REPLACE FUNCTION pager_get_board_data(
    p_website_id INTEGER,
    p_company_id INTEGER
) RETURNS JSONB AS $$
DECLARE
    v_active JSONB;
    v_resolved JSONB;
    v_stats JSONB;
    v_on_duty VARCHAR;
BEGIN
    -- Get current on-duty user name
    SELECT r.name INTO v_on_duty
    FROM calendar_event e
    JOIN res_users u ON e.user_id = u.id
    JOIN res_partner r ON u.partner_id = r.id
    WHERE e.is_pager_duty = TRUE
      AND e.start <= NOW()
      AND e.stop >= NOW()
      AND (
          e.website_id = p_website_id
          OR (
              e.website_id IS NULL
              AND p_website_id IS NULL
          )
      )
      AND (
          u.company_id = p_company_id
          OR (
              u.company_id IS NULL
              AND p_company_id IS NULL
          )
      )
    LIMIT 1;

    -- Get active incidents (capped at 50)
    SELECT jsonb_agg(t) INTO v_active FROM (
        SELECT
            name, source, severity, status,
            create_date,
            (
                SELECT r.name
                FROM res_users u
                JOIN res_partner r
                    ON u.partner_id = r.id
                WHERE u.id = i.acknowledged_by_id
            ) as ack_name
        FROM pager_incident i
        WHERE status IN ('open', 'acknowledged')
          AND (
              website_id = p_website_id
              OR (
                  website_id IS NULL
                  AND p_website_id IS NULL
              )
          )
          AND (
              company_id = p_company_id
              OR (
                  company_id IS NULL
                  AND p_company_id IS NULL
              )
          )
        ORDER BY create_date DESC
        LIMIT 50
    ) t;

    -- Get resolved incidents (capped at 10)
    SELECT jsonb_agg(t) INTO v_resolved FROM (
        SELECT name, source, severity, time_resolved
        FROM pager_incident
        WHERE status = 'resolved'
          AND (
              website_id = p_website_id
              OR (
                  website_id IS NULL
                  AND p_website_id IS NULL
              )
          )
          AND (
              company_id = p_company_id
              OR (
                  company_id IS NULL
                  AND p_company_id IS NULL
              )
          )
        ORDER BY time_resolved DESC
        LIMIT 10
    ) t;

    -- Get stats
    SELECT jsonb_object_agg(status, count)
    INTO v_stats FROM (
        SELECT status, count(*) as count
        FROM pager_check
        WHERE (
            website_id = p_website_id
            OR (
                website_id IS NULL
                AND p_website_id IS NULL
            )
        )
        AND (
            company_id = p_company_id
            OR (
                company_id IS NULL
                AND p_company_id IS NULL
            )
        )
        GROUP BY status
    ) t;

    RETURN jsonb_build_object(
        'on_duty', COALESCE(v_on_duty, 'None'),
        'active', COALESCE(v_active, '[]'::jsonb),
        'resolved',
        COALESCE(v_resolved, '[]'::jsonb),
        'stats', jsonb_build_object(
            'passing',
            COALESCE((v_stats->>'passing')::int, 0),
            'failing',
            COALESCE((v_stats->>'failing')::int, 0),
            'maintenance',
            COALESCE(
                (v_stats->>'maintenance')::int, 0
            )
        )
    );
END;
$$ LANGUAGE plpgsql;
"""


def _install_postgres_procedures(env):
    """
    Install Postgres stored procedures for pager board.
    # Verified by [@ANCHOR: test_pager_duty_procedures]
    """
    env.flush_all()
    try:
        with env.cr.savepoint():
            env.cr.execute(_PAGER_BOARD_SQL)
    except Exception as e:  # audit-ignore-catch-all
        _logger.error(
            "Failed to install pager_get_board_data: %s",
            e,
        )


def post_init_hook(env):
    """
    Register daemon keys and trigger autodiscovery upon installation.
    """
    # [@ANCHOR: pager_duty_postgres_procedures]
    _install_postgres_procedures(env)

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
