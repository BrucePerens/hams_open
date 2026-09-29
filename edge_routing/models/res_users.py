# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

from odoo import models, fields


class ResUsersEdgeRouting(models.Model):
    name = fields.Char(string="Name", required=True)
    """
    Extends res.users with edge.routing.mixin to provide high-performance
    vanity URL routing and slug caching.
    """

    _name = "res.users"
    _inherit = ["res.users", "edge.routing.mixin"]

    # get_record_by_slug() used to be overridden here to add a fallback
    # that matched the URL slug against res.users.login when no
    # website_slug matched. Removed (2026-09-24): every real human
    # account's login is now an email address (see git history for
    # "invite flow must set login=email"), which turned that fallback
    # into a live PII exposure -- a public, unauthenticated route that
    # let anyone probe hams.com/<email> to learn whether an account
    # exists for that address and get redirected to its profile. Worse,
    # SWL (Short Wave Listener) accounts never get a website_slug at all
    # (see ham_onboarding/models/res_users_invite.py's
    # _activate_invited_member, which only sets it in the
    # operator_type == "ham" branch), so an SWL's email-shaped login was
    # the ONLY thing this fallback could ever match for them -- the
    # fallback existing at all left SWL accounts effectively unprotected
    # by it.
    #
    # res.users now resolves purely through edge.routing.mixin's own
    # get_record_by_slug() (website_slug only), inherited unmodified.
    # Real ham accounts get a real website_slug from their callsign at
    # signup or invite redemption; SWL and service/system accounts are
    # simply not reachable by a public vanity-URL guess, which is the
    # safer default -- no vanity URL for them, rather than a leaky
    # fallback. This also means the mixin's own @distributed_cache()
    # now covers res.users too: the old override was deliberately left
    # uncached because the login-fallback branch wasn't covered by
    # write()'s website_slug/name-only cache-invalidation hook, and that
    # reason no longer applies.
