# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import api, models
from odoo.http import request
from odoo.service import security


class IrHttp(models.AbstractModel):
    _inherit = "ir.http"

    @classmethod
    def _match(cls, path):
        # [@ANCHOR: hams_base_validate_session_before_website_match]
        """Validate the session before website's URL matching reads records with it.

        Odoo core's ``Request._serve_db`` builds ``request.env`` from the raw,
        unvalidated ``session.uid`` and then calls ``ir.http._match``; the
        session is only checked (and a dead one logged out) later, in
        ``_authenticate_explicit``. website/http_routing's ``_match`` reads
        ``website`` and ``res.lang`` records in that window, under the
        unvalidated uid, and http_routing's "temporarily grant the public
        user" step is skipped because ``session.uid`` is set. A session whose
        user has since been deleted therefore fails the ACL check there
        (``Access Denied by ACLs ... uid: <deleted id>, model: website``) and
        the visitor gets a bare 403 on every frontend page, instead of being
        logged out and served as the public user. Seen on hams.com production
        2026-09-23/24 as 35 hits for a nonexistent ``uid: 99``.

        This runs the same check ``_authenticate_explicit`` runs, earlier.
        ``request`` is deliberately not passed to ``check_session``: the
        cursor is still read-only here, and ``_authenticate_explicit`` records
        the device log for a valid session afterwards as usual. The later
        call is an ormcache hit on ``_compute_session_token``.
        """
        if request and request.session.uid is not None:
            if not security.check_session(request.session, request.env):
                request.session.logout(keep_db=True)
                request.env = api.Environment(
                    request.env.cr, None, request.session.context
                )
        return super()._match(path)
