# Copyright © HAMS project. AGPL-3.0-or-later.
{
    "name": "Site-Wide Advertising",
    "version": "1.0",
    "author": "Bruce Perens K6BP",
    "category": "Website",
    "summary": "Google AdSense integration, consent-gated and off by default",
    "description": (
        "Adds an optional, consent-gated AdSense placement to the shared website "
        "layout, per docs/proposals/ADVERTISING.md. Renders nothing at all until "
        "an administrator configures a real AdSense client ID and ad slot ID -- "
        "there is no default 'on' state. Ad personalization is gated on the same "
        "cookies-bar consent signal (optionalCookiesAccepted/optionalCookiesDenied) "
        "core website.google_analytics_key already uses, independent of whether "
        "Google Analytics is itself configured on a given site."
    ),
    "depends": ["base", "website", "zero_sudo"],
    "data": [
        "views/website_layout.xml",
        "views/res_config_settings_views.xml",
    ],
    "installable": True,
    "application": False,
    "license": "AGPL-3",
}
