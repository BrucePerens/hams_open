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
          p_company_id IS NULL
          OR EXISTS (
              -- Bug-hunt fix: this used to check u.company_id (res.users'
              -- own SINGLE default company), not whether the on-duty
              -- user is actually a MEMBER of the requesting company --
              -- the wrong field for this codebase's own established
              -- multi-company convention (see e.g. this module's
              -- pager_check_website_company_rule ir.rule, which checks
              -- company_id IN company_ids, and hams_shared/docs/
              -- odoo_orm_reference.md's own note that company_ids/
              -- _get_company_ids() is the real source of a user's
              -- granted company scope). An admin whose personal default
              -- company is A but who also covers company B (in their
              -- company_ids) would silently show as "no one on duty" on
              -- B's dashboard under the old check even while genuinely
              -- on shift for B.
              SELECT 1 FROM res_company_users_rel rcu
              WHERE rcu.user_id = u.id AND rcu.cid = p_company_id
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
    # [@ANCHOR: pager_duty:install_postgres_procedures]
    env.flush_all()
    try:
        with env.cr.savepoint():
            env.cr.execute(_PAGER_BOARD_SQL)
    except Exception as e:  # audit-ignore-catch-all
        _logger.error(
            "Failed to install pager_get_board_data: %s",
            e,
        )


def _claim_info_alias(env):
    """Claim the "info" mail alias for pager.incident. Same collision this
    codebase already worked around for hams_helpdesk: stock Odoo's own crm
    module ships a built-in (non-demo) "info" alias on its default Sales
    Team (crm/data/crm_team_data.xml), and mail.alias.alias_name is
    globally unique, so a plain data/mail_alias_data.xml <record> for
    "info" would hard-crash this module's own install the instant crm is
    present. info@hams.com routes to pager_duty (not hams_helpdesk.ticket
    or crm's default Sales Team) per Bruce's own direction -- hams_helpdesk
    no longer claims it.

    Real bug found live on hams1, 2026-09-22: crm was installed before
    this hook ever ran (post_init_hook only fires on a fresh module
    install, so an already-installed pager_duty never got a second
    chance), so crm's own team_sales_department claimed "info" first and
    this function's original "skip if anything already has it" logic
    silently left info@hams.com routing to crm.lead instead of
    pager.incident -- exactly the documented risk, now real, and it broke
    the SES inbound-mail daemon outright: crm.lead's own message_new()
    path reads crm.pls_fields (a core system parameter), which the
    daemon's narrowly-scoped mail_ingest_service_internal account is
    correctly forbidden from reading, so every info@ email crashed
    ingestion instead of creating a lead. Specifically detects and
    reclaims exactly this one collision (crm's default Sales Team, not
    just "whoever has it") rather than unconditionally stealing the
    alias from an unrelated customization.
    """
    # [@ANCHOR: pager_duty_info_alias_claim]
    existing = env["mail.alias"].search([("alias_name", "=", "info")], limit=1)
    if existing:
        sales_team = env.ref("sales_team.team_sales_department", raise_if_not_found=False)
        crm_team_model = env.ref("crm.model_crm_team", raise_if_not_found=False)
        if (
            sales_team
            and crm_team_model
            and existing.alias_parent_model_id == crm_team_model
            and existing.alias_parent_thread_id == sales_team.id
        ):
            _logger.warning(
                "pager_duty: reclaiming the 'info' mail alias from crm's "
                "default Sales Team (id %s) for pager.incident, per Bruce's "
                "own direction that info@hams.com routes here.",
                sales_team.id,
            )
            # Release it through crm.team's own field first -- this is the
            # same code path Odoo itself uses to manage this alias, so it
            # correctly clears alias_parent_model_id/alias_parent_thread_id
            # too, not just alias_name.
            sales_team.write({"alias_name": False})
        else:
            _logger.warning(
                "pager_duty: the 'info' mail alias already belongs to another "
                "record (parent model id %s, thread id %s); info@hams.com "
                "will NOT route to pager.incident.",
                existing.alias_parent_model_id.id if existing.alias_parent_model_id else None,
                existing.alias_parent_thread_id,
            )
            return
    env["mail.alias"].create(
        {
            "alias_name": "info",
            "alias_model_id": env.ref("pager_duty.model_pager_incident").id,
            "alias_contact": "everyone",
            "alias_defaults": "{'severity': 'low', 'incident_source_prefix': 'info-email'}",
        }
    )


def post_init_hook(env):
    """
    Register daemon keys and trigger autodiscovery upon installation.
    """
    # [@ANCHOR: pager_duty_postgres_procedures]
    _install_postgres_procedures(env)

    _claim_info_alias(env)

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
    # Registered as pager_duty's own service account, which register_daemon()
    # authorizes to provision a key for itself -- not as base.user_admin, which
    # this hook used until 2026-09-16 against the zero-sudo design.
    # Verified by [@ANCHOR: pager_duty:test_post_init_hook_registers_as_own_service_account]
    if "daemon.key.registry" in env:
        svc_uid = env["zero_sudo.security.utils"]._get_service_uid(
            "pager_duty.user_pager_service_internal"
        )
        env["daemon.key.registry"].with_user(svc_uid).register_daemon(
            daemon_name="Pager Duty - Generalized Monitor",
            user_xml_id="pager_duty.user_pager_service_internal",
            env_file_path="/opt/hams/etc/keys/pager_duty.env",
        )
