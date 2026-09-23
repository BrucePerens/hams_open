# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
{
    "name": "Content Moderation",
    "summary": "Generic content-violation report/track/enforce pipeline, content-type-agnostic.",
    "description": """
Provides `content.violation.report`: anyone (public or portal) can report a
URL/target for review; staff (this module's own "Moderator" role) can mark a
report under review, dismiss it, or take action.

What "take action" actually DOES to the reported content is deliberately
left to an extension hook -- `_apply_enforcement_action()`, overridable by
any module via `_inherit = "content.violation.report"` -- because the real
consequence (unpublish a personal website, mute a forum account, delist a
classifieds ad, ...) is different for every kind of content, and this base
module has no way to know which one it's talking to. This module's own
default is a safe, documented no-op: see that method's docstring in
models/content_violation_report.py for exactly why (it would otherwise
assume fields/procedures that only exist once a consuming module installs
them).

Extracted 2026-09-23 out of `user_websites`, which used to own a version of
this same mechanism hard-wired to its own personal/group website suspension
logic (see `user_websites`'s own content_violation_report_moderation.py for
that override). Extracted so other hams_com content surfaces -- forum
posts, classifieds listings, private messages, voice-QSO recordings -- can
reuse the same report/track/enforce pipeline without a hard dependency on
all of `user_websites` (which they have nothing to do with) or duplicating
this mechanism per feature. See hams_com's
docs/proposals/CHILD_SAFETY_COMMUNICATIONS_CONSENT.md for the concrete
near-term consumers this was extracted for; none of them are wired up by
this extraction itself, only unblocked.
    """,
    "author": "Bruce Perens K6BP",
    "website": "https://perens.com/",
    "category": "Website",
    "version": "1.0",
    "license": "AGPL-3",
    "depends": [
        "base",
        "mail",
        "portal",
        "zero_sudo",
    ],
    "data": [
        "security/content_moderation_security.xml",
        "security/ir.model.access.csv",
        "data/content_moderation_data.xml",
        "views/content_violation_report_views.xml",
    ],
    "demo": [],
    "installable": True,
    "application": False,
}
