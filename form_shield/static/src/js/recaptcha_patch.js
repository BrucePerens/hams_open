/** @odoo-module **/
/* SPDX-License-Identifier: AGPL-3.0-or-later */

import { patch } from "@web/core/utils/patch";
import { _t } from "@web/core/l10n/translation";
import { session } from "@web/session";
import { ReCaptcha } from "@google_recaptcha/js/recaptcha";
import { ShieldSession } from "@form_shield/js/shield_client";

// [@ANCHOR: form_shield:recaptcha_patch]
// Verified by [@ANCHOR: form_shield:test_odoo_forms_require_a_solved_challenge]
//
// Odoo's own forms (website form builder, event registration, newsletter subscribe, follow) ask
// `new ReCaptcha()` for a token named by the form's action and send it as recaptcha_token_response.
// This replaces what stands behind that class: no Google script is loaded, and the token is the
// shield's `<challenge>~<counter>`. The challenge is fetched when the form wakes up (so the
// server's minimum time-to-submit runs while the visitor fills the form in) and solved when the
// form asks for its token, which is when the action name, and so the form, is known.
// The actions the server enforces (models/ir_http.py ENFORCED_ACTIONS; keep the two lists equal).
// For any other action, such as "login", the form is not slowed down: it gets what stock Odoo gives
// a site with no reCAPTCHA key, an answer with no token.
export const ENFORCED_ACTIONS = new Set([
    "website_form",
    "website_event_registration",
    "website_mass_mailing_subscribe",
    "website_mail_follow",
]);

patch(ReCaptcha.prototype, {
    loadLibs() {
        if (session.is_website_user) {
            this._shield = new ShieldSession(null);
        }
        return Promise.resolve(true);
    },

    async getToken(action) {
        if (!ENFORCED_ACTIONS.has(action) || !session.is_website_user) {
            return { message: _t("No automatic check is asked for on this form.") };
        }
        if (!this._shield) {
            this._shield = new ShieldSession(null);
        }
        const session = this._shield;
        try {
            const token = await session.token(action);
            // One answer, one use: the next submit from this form needs a challenge of its own.
            this._shield = new ShieldSession(null);
            return { token };
        } catch {
            this._shield = new ShieldSession(null);
            return {
                error: _t("The automatic check could not run in this browser. Please try again."),
            };
        }
    },
});
