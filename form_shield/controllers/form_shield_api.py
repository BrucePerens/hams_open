# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import http
from odoo.http import request

from .. import tokens


class FormShieldController(http.Controller):

    # [@ANCHOR: form_shield:challenge_route]
    # Verified by [@ANCHOR: form_shield:test_challenge_endpoint_issues_signed_uncacheable_json]
    # A POST that returns `Cache-Control: no-store` JSON, never a GET: the pages that call it
    # may come from Cloudflare's edge cache, and each visitor needs a challenge of their own
    # (bound to their network, one use). It sets no cookie and writes nothing, so it needs no
    # session and no CSRF token (csrf=False): all it can do is hand out a signed string.
    @http.route(
        "/form_shield/challenge",
        type="http",
        auth="public",
        methods=["POST"],
        csrf=False,
        readonly=True,
        save_session=False,
        sitemap=False,
    )
    def challenge(self, kind="pow", **_ignored):
        wanted = tokens.KIND_QUESTION if kind == "q" else tokens.KIND_POW
        issued = request.env["form.shield"].issue(wanted)
        return request.make_json_response(
            issued, headers=[("Cache-Control", "no-store")]
        )
