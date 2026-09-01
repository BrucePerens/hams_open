# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
from odoo import fields, models


class Website(models.Model):
    _inherit = "website"
    # This model is multi-tenant and multi-website, matching caching's own
    # per-website field pattern -- each website can carry its own AdSense
    # configuration (or none at all).

    google_adsense_client_id = fields.Char(
        string="AdSense Publisher ID",
        help=(
            "Google AdSense publisher ID (e.g. ca-pub-XXXXXXXXXXXXXXXX). Leave "
            "empty to serve no ads at all -- this is the default, and nothing "
            "in the site layout renders any ad-related markup or script until "
            "this is set."
        ),
    )

    google_adsense_footer_slot_id = fields.Char(
        string="AdSense Footer Ad Slot ID",
        help=(
            "The AdSense ad-unit slot ID for the single footer banner placement "
            "(docs/proposals/ADVERTISING.md's recommended minimal starting "
            "point). Both this and the Publisher ID above must be set for the "
            "footer ad to render -- setting only one renders nothing."
        ),
    )

    google_adsense_sidebar_slot_id = fields.Char(
        string="AdSense Sidebar Ad Slot ID",
        help=(
            "The AdSense ad-unit slot ID for a fixed-position sidebar "
            "placement, per Bruce's own 2026-09-01 direction ('a sidebar and "
            "footer per page'). Renders as a fixed skyscraper unit on the "
            "page's right edge on wide viewports only (hidden below the "
            "Bootstrap xl breakpoint, so it never competes with content on "
            "narrow screens). Both this and the Publisher ID above must be "
            "set for the sidebar ad to render -- setting only one renders "
            "nothing."
        ),
    )
