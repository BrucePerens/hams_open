# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. License: AGPL-3.0.
from odoo import api, models
from odoo.http import request
from odoo.tools import SQL


class WebsiteVisitor(models.Model):
    _inherit = "website.visitor"

    # Same window as core's own `_add_tracking` ("a track at most every 30 minutes").
    TRACK_DEDUP_MINUTES = 30

    # [@ANCHOR: hams_base:visitor_track_dedup]
    @api.model
    def _handle_webpage_dispatch(self, website_page):
        """Do not UPSERT the visitor and INSERT a website.track row again for a page this visitor already viewed.

        Core runs `_upsert_visitor` plus a `website_track` INSERT on every 200 response of a tracked page (/event,
        /blog ...). The visitor is one row per session, so requests that share a session (a crawler, an embedded
        client, a browser with many tabs, the 40 virtual users of the hams1 readiness audit load test) all write the
        same row at once. PostgreSQL answers with a serialization failure, Odoo retries the whole request (110
        queries became 207, 304, 401, 499: one more full copy of the request per retry) and after five tries the
        request is a 500. The same path also adds one `website_track` row per page view without bound.

        A read does not conflict, so check first whether this session already has a track for this exact URL
        within `TRACK_DEDUP_MINUTES` and, if so, write nothing."""
        if self._track_exists_for_this_request():
            return None
        return super()._handle_webpage_dispatch(website_page)

    @api.model
    def _track_exists_for_this_request(self):
        try:
            token = self._get_access_token()
        except ValueError:
            return False
        self.env.cr.execute(SQL(
            """
            SELECT 1
              FROM website_track t
              JOIN website_visitor v ON v.id = t.visitor_id
             WHERE v.access_token = %(token)s
               AND t.url = %(url)s
               AND t.visit_datetime > (now() at time zone 'UTC') - make_interval(mins => %(minutes)s)
             LIMIT 1
            """,
            token=str(token), url=request.httprequest.url, minutes=self.TRACK_DEDUP_MINUTES,
        ))
        return bool(self.env.cr.fetchone())
