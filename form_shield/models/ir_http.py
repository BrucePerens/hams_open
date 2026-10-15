# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import api, models
from odoo.exceptions import UserError
from odoo.http import request
from odoo.tools.translate import LazyTranslate

from .form_shield import (
    LIMITED,
    QUESTION,
    RETRY,
    SILENT,
    UNAVAILABLE,
)

_lt = LazyTranslate(__name__)

# Odoo's own forms ask for a captcha by action name (`captcha=` on the route, or data-captcha on
# the form). Each of these carries the token our patch of the ReCaptcha class produces
# (static/src/js/recaptcha_patch.js). "login" is deliberately absent: see the module README.
ENFORCED_ACTIONS = (
    "website_form",
    "website_event_registration",
    "website_mass_mailing_subscribe",
    "website_mail_follow",
)

MESSAGES = {
    SILENT: _lt("We could not verify this submission. Please reload the page and try again."),
    RETRY: _lt("The check on this form timed out. Please try again."),
    QUESTION: _lt("Please reload the page, with scripts enabled, and try again."),
    LIMITED: _lt("Too many submissions from your network. Please wait a while and try again."),
    UNAVAILABLE: _lt("This form is temporarily unavailable. Please try again later."),
}


class IrHttp(models.AbstractModel):
    _inherit = "ir.http"

    # [@ANCHOR: form_shield:captcha_hook]
    # Verified by [@ANCHOR: form_shield:test_odoo_forms_require_a_solved_challenge]
    @api.model
    def _verify_request_recaptcha_token(self, action):
        """Replaces Google reCAPTCHA for the actions above with the self-hosted shield.

        The token field keeps its stock name (`recaptcha_token_response`) because the stock
        forms already send whatever ReCaptcha.getToken() returns under it. For other actions
        the stock behaviour (which does nothing unless reCAPTCHA keys are configured) stands.
        """
        if action not in ENFORCED_ACTIONS:
            return super()._verify_request_recaptcha_token(action)
        response = request.params.pop("recaptcha_token_response", "")
        shield = self.env["form.shield"]
        # The count is written on its own cursor: raising below rolls this request back.
        verdict = shield.check(action, response, own_cursor=True)
        if verdict.ok:
            return None
        raise UserError(str(MESSAGES[verdict.outcome]))
