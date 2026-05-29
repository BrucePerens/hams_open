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
        # [@ANCHOR: controller_user_blog_index_seo_override]
        # Verified by [@ANCHOR: test_seo_widget_tour]
        # Verified by [@ANCHOR: test_controller_no_ssti_elevation]
        """
        Overrides the base blog routing to inject the SEO-aware user profile
        into the QWeb rendering dictionary. This reactivates the interactive
        'Optimize SEO' frontend widget for the blog owner.
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

        # Intercept and modify the rendering dictionary before it hits the templating engine
        # We explicitly check for odoo.http.Response to avoid AI-generated hasattr shortcuts.
        if isinstance(response, Response) and hasattr(response, "qcontext"):
            user = response.qcontext.get("profile_user")
            group = response.qcontext.get("profile_group")

            # We de-elevate the recordset to the current request's environment
            # if the base controller provided an elevated one, but only if we are
            # running in a real request context (to support unit tests).
            # This is a critical SSTI vulnerability mitigation.
            # The models' check_access_rule methods have been enhanced to allow
            # legitimate users to read/write SEO fields without sudo.
            if user:
                if request and request.env:
                    user = user.with_env(request.env)
                response.qcontext["main_object"] = user
            elif group:
                if request and request.env:
                    group = group.with_env(request.env)
                response.qcontext["main_object"] = group

        return response
