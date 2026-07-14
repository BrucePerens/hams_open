# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
from odoo import http
from odoo.http import request, Response
from odoo.addons.user_websites.controllers.main import UserWebsitesController


class UserWebsitesSEOController(UserWebsitesController):

    @http.route(
        ["/<string:website_slug>/blog", "/<string:website_slug>/blog/"],
        type="http",
        auth="public",
        website=True,
    )
    def user_blog_index(
        self,
        website_slug,
        tag=None,
        search=None,
        date_begin=None,
        date_end=None,
        page=1,
        **kwargs
    ):
        # [@ANCHOR: COMM_controller_user_blog_index_seo_override]

        # Verified by [@ANCHOR: COMM_test_seo_widget_tour]

        # Verified by [@ANCHOR: COMM_test_controller_no_ssti_elevation]
        """
        Overrides the base blog routing to inject the SEO-aware user profile
        into the QWeb rendering dictionary.
        """
        # Execute the base controller logic
        response = super().user_blog_index(
            website_slug,
            tag=tag,
            search=search,
            date_begin=date_begin,
            date_end=date_end,
            page=page,
            **kwargs
        )

        # Intercept and modify the rendering dictionary
        if isinstance(response, Response):
            qcontext = response.qcontext
            if qcontext is None:
                return response

            user = qcontext.get("profile_user")
            group = qcontext.get("profile_group")

            # ADR-0078: Pre-fetch SEO fields using the existing service account environment
            # This primes the ORM cache safely before we de-elevate the recordset.
            if user:
                user.read(list(user._get_seo_fields()))
                # We de-elevate the recordset to the current request's environment to prevent SSTI
                if request and request.env:
                    user = user.with_user(request.env.user)
                qcontext["main_object"] = user
                qcontext["profile_user"] = user
            elif group:
                group.read(list(group._get_seo_fields()))
                # We de-elevate the recordset to the current request's environment to prevent SSTI
                if request and request.env:
                    group = group.with_user(request.env.user)
                qcontext["main_object"] = group
                qcontext["profile_group"] = group

        return response
