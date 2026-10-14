# SPDX-License-Identifier: AGPL-3.0-or-later
{
    "name": "Form Shield",
    "version": "1.0",
    "summary": "Self-hosted, privacy-preserving anti-spam for public forms (no third-party captcha)",
    "description": """
A proof-of-work challenge solved by the browser in a Web Worker (no interaction, no image, no
third party), a plain-text question for browsers that cannot run it, a honeypot, a minimum
time-to-submit, and per-network, per-target and per-form rate limits. Replaces Google reCAPTCHA
for Odoo's own website forms. Counts refusals per day, with no address or text kept.
    """,
    "author": "Bruce Perens K6BP",
    "website": "https://perens.com/",
    "category": "Website",
    "license": "AGPL-3",
    "depends": [
        "base",
        "website",
        "google_recaptcha",
        "zero_sudo",
        "distributed_redis_cache",
        "knowledge",
        "cloudflare",
    ],
    "external_dependencies": {"python": ["redis"]},
    "data": [
        "security/form_shield_security.xml",
        "security/ir.model.access.csv",
        "views/form_shield_views.xml",
        "views/form_shield_templates.xml",
    ],
    "knowledge_docs": [
        {
            "name": "Form Shield",
            "path": "data/documentation.html",
            "icon": "S",
            "category": "workspace",
            "public": True,
        }
    ],
    "assets": {
        "web.assets_frontend": [
            "form_shield/static/src/pow/pow_core.js",
            "form_shield/static/src/js/shield_client.js",
            "form_shield/static/src/js/form_shield_form.js",
            "form_shield/static/src/js/recaptcha_patch.js",
            "form_shield/static/src/js/error_notice.js",
            "form_shield/static/src/css/form_shield.css",
        ],
        "web.assets_unit_tests": [
            "form_shield/static/src/pow/pow_core.js",
            "form_shield/static/src/js/shield_client.js",
            "form_shield/static/src/js/error_notice.js",
            "form_shield/static/tests/form_shield.test.js",
        ],
    },
    "post_init_hook": "post_init_hook",
    "installable": True,
    "application": False,
}
