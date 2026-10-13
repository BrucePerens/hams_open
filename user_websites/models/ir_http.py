# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).
"""The site-slug route converter: a slug is never an installed language's code.

/<slug> routes see every one-segment path (/<slug>, /<slug>/blog, /<slug>/home, ...). Odoo's own
language handling (http_routing ir.http._match) only runs for a path no route matched, so a slug
route that matched /en_US/ or /fr/blog would hide the language prefix and answer 404 (found
2026-10-13: /en_US/ broke the ham_init tours). Raising ValidationError from to_python() makes
werkzeug treat the rule as not matching, so those paths reach the language handling as before.
The list of language codes is read at match time, so installing a language needs no restart.
"""
from werkzeug.routing import ValidationError
from werkzeug.routing.converters import UnicodeConverter

from odoo import models
from odoo.http import request

from odoo.addons.edge_routing.utils import language_slugs


class SiteSlugConverter(UnicodeConverter):
    # [@ANCHOR: user_websites:site_slug_converter]
    # Verified by [@ANCHOR: test_language_prefixed_paths_still_work]

    def to_python(self, value):
        # [@ANCHOR: user_websites:fn_site_slug_converter_to_python]
        if request and request.env and value.lower() in language_slugs(request.env):
            raise ValidationError()
        return super().to_python(value)


class IrHttp(models.AbstractModel):
    _inherit = "ir.http"

    @classmethod
    def _get_converters(cls):
        # [@ANCHOR: user_websites:fn_ir_http_get_converters]
        return {**super()._get_converters(), "uwslug": SiteSlugConverter}
