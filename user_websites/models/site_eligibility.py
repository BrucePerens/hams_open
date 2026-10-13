# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).
"""Who may have a member website and blog: an extension point, open to everyone by default.

[@ANCHOR: user_websites:site_eligibility]
Verified by [@ANCHOR: test_site_eligibility_hook]

Open source: every account is eligible, nothing here changes anything. A deployment narrows it by
overriding ResUsers._website_eligibility_gaps(), which returns one dict per missing requirement:
{"code": ..., "message": ..., "url": ...} (url: where the member fixes it, or ""). hams.com's
override (ham_communications_consent) asks for a Verified Ham who has accepted the current agreement
and completed the $1 verification; its demo mode (ham_demo_mode) waives both.

What ineligibility does, and what it never does:
  * the member's pages, blog and posts stop being public. Public routes ask the live answer
    (_website_publicly_visible) and answer 404; reconcile_website_eligibility() also unpublishes the
    content, marking each piece hidden_by_eligibility, so the stock blog, search, RSS and sitemap
    drop it too, and republishes exactly the marked pieces when the member is eligible again. A
    page the member unpublished themselves is never republished.
  * nothing is deleted. The owner's tools explain what is missing instead of answering 404.
  * res.users.website_ineligible mirrors the last reconciled answer for the SQL views (directory,
    slug routing). Nobody can write it from the web: it is not in SELF_WRITEABLE_FIELDS.
  * no new page, post, blog or image is accepted while ineligible.
Administrators and internal users have no exemption on the public site.
"""
import logging

from odoo import api, fields, models

_logger = logging.getLogger(__name__)

SVC = "user_websites.user_websites_service_account"


class ResUsersSiteEligibility(models.Model):
    _inherit = "res.users"

    website_ineligible = fields.Boolean(
        string="Website Not Allowed",
        default=False,
        copy=False,
        readonly=True,
        index=True,
        help="The last reconciled answer of _website_eligibility_gaps(): True while the member's site is "
        "hidden because a requirement is missing. Maintained by reconcile_website_eligibility(); never "
        "set by the member.",
    )

    # [@ANCHOR: user_websites:COMM_website_eligibility_gaps]
    def _website_eligibility_gaps(self):
        """What this member still needs before they may have a website and blog: a list of
        {"code", "message", "url"} dicts, empty when eligible. Open source: nothing is required."""
        self.ensure_one()
        return []

    # [@ANCHOR: user_websites:COMM_hams_website_eligible]
    def _hams_website_eligible(self):
        self.ensure_one()
        return not self._website_eligibility_gaps()

    # [@ANCHOR: user_websites:COMM_website_publicly_visible]
    def _website_publicly_visible(self):
        """Whether visitors may see this member's site right now (not suspended, and eligible)."""
        self.ensure_one()
        return not self.is_suspended_from_websites and self._hams_website_eligible()

    @api.model
    def _website_site_user_ids(self):
        """Members who have a site address (the only ones eligibility can hide)."""
        self.env.cr.execute(  # audit-ignore-sql: static SQL, no parameters
            "SELECT id FROM res_users WHERE active AND website_slug IS NOT NULL AND website_slug <> '' "
            "AND is_service_account IS NOT TRUE"
        )
        return [row[0] for row in self.env.cr.fetchall()]

    # [@ANCHOR: user_websites:COMM_reconcile_website_eligibility]
    # Verified by [@ANCHOR: test_site_eligibility_hook]
    @api.model
    def reconcile_website_eligibility(self, user_ids=None):
        """Bring hidden content and website_ineligible in line with the live answer. Returns
        {user_id: "hidden" | "shown"} for the members that changed. Idempotent; run by the cron, by
        the hams.com state-change hooks, and lazily by the owner dashboard."""
        env_svc = self.env["zero_sudo.security.utils"]._get_service_env(SVC)
        ids = user_ids if user_ids is not None else self._website_site_user_ids()
        changed = {}
        for user in env_svc["res.users"].browse(ids).exists():
            ineligible = bool(user._website_eligibility_gaps())
            if ineligible and not user.website_ineligible:
                self._hide_site_content(env_svc, user)
                user.write({"website_ineligible": True})
                changed[user.id] = "hidden"
            elif not ineligible and user.website_ineligible:
                self._show_site_content(env_svc, user)
                user.write({"website_ineligible": False})
                changed[user.id] = "shown"
            elif ineligible:
                # New content published since the last run (a race with an in-flight request).
                self._hide_site_content(env_svc, user)
        if changed:
            _logger.info("user_websites: site eligibility changed: %s", changed)
        return changed

    @api.model
    def _hide_site_content(self, env_svc, user):
        pages = env_svc["website.page"].search([("owner_user_id", "=", user.id), ("website_published", "=", True)])
        if pages:
            pages.write({"website_published": False, "hidden_by_eligibility": True})
        posts = env_svc["blog.post"].search([("owner_user_id", "=", user.id), ("is_published", "=", True)])
        if posts:
            posts.write({"is_published": False, "hidden_by_eligibility": True})

    @api.model
    def _show_site_content(self, env_svc, user):
        if user.is_suspended_from_websites:
            return
        pages = env_svc["website.page"].search([("owner_user_id", "=", user.id), ("hidden_by_eligibility", "=", True)])
        if pages:
            pages.write({"website_published": True, "hidden_by_eligibility": False})
        posts = env_svc["blog.post"].search([("owner_user_id", "=", user.id), ("hidden_by_eligibility", "=", True)])
        if posts:
            posts.write({"is_published": True, "hidden_by_eligibility": False})

    # [@ANCHOR: user_websites:COMM_cron_reconcile_website_eligibility]
    @api.model
    def _cron_reconcile_website_eligibility(self):
        return self.reconcile_website_eligibility()

