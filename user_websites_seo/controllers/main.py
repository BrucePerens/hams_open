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
            qcontext = getattr(response, "qcontext", None)
            if qcontext is None:
                return response

            user = qcontext.get("profile_user")
            group = qcontext.get("profile_group")

            # We de-elevate the recordset to the current request's environment
            if user:
                if request and request.env:
                    user = user.with_env(request.env)
                # ADR-0078: Pre-fetch SEO fields
                # micro-privilege: use facility service account so public users can resolve the SEO fields
                # Use user.env to avoid RuntimeError: object is not bound on request.env in tests
                svc_uid = user.env["zero_sudo.security.utils"]._get_service_uid("zero_sudo.odoo_facility_service_internal")
                user_svc = user.with_user(svc_uid)
                user_svc.read(list(user_svc._get_seo_fields()))
                qcontext["main_object"] = user_svc
            elif group:
                if request and request.env:
                    group = group.with_env(request.env)
                # ADR-0078: Pre-fetch SEO fields
                svc_uid = group.env["zero_sudo.security.utils"]._get_service_uid("zero_sudo.odoo_facility_service_internal")
                group_svc = group.with_user(svc_uid)
                group_svc.read(list(group_svc._get_seo_fields()))
                qcontext["main_object"] = group_svc

        return response
