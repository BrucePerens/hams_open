/** @odoo-module **/
/* SPDX-License-Identifier: AGPL-3.0-or-later */

import { Interaction } from "@web/public/interaction";
import { registry } from "@web/core/registry";
import { _t } from "@web/core/l10n/translation";

// [@ANCHOR: form_shield:error_notice]
// Verified by [@ANCHOR: form_shield:test_error_notice_messages]
//
// A shielded form that refuses a person redirects back to its page with ?form_shield_error=<reason>.
// This says what happened in Odoo's notification (announced to screen readers) and removes the
// parameter so a reload does not repeat it. Reasons that identify a program are never redirected
// with: those get the form's ordinary success answer.
// Lazy translations, built once: _t() at module level is translated when first shown.
const MESSAGES = {
        expired: _t("The check on the form timed out. Please open it again and resend."),
        ip_changed: _t("Your network changed while you filled in the form. Please open it again and resend."),
        too_fast: _t("That was sent very quickly. Please wait a moment and send it again."),
        wrong_answer: _t("That answer was not right. Please try again."),
        no_token: _t("The check did not run. Please reload the page, with scripts enabled, and try again."),
        rate_subject: _t("Too many submissions from your network. Please wait a while and try again."),
        rate_target: _t("This has been reported many times today. Please try again tomorrow."),
        rate_global: _t("This form is very busy. Please try again later."),
        unavailable: _t("This form is temporarily unavailable. Please try again later."),
};
const DEFAULT_MESSAGE = _t("We could not send the form. Please try again.");

// [@ANCHOR: form_shield:notice_text]
// Verified by [@ANCHOR: form_shield:test_error_notice_messages]
export function noticeText(reason) {
    return MESSAGES[reason] || DEFAULT_MESSAGE;
}

export class FormShieldErrorNotice extends Interaction {
    static selector = "#wrapwrap";

    start() {
        const params = new URLSearchParams(document.location.search);
        if (!params.has("form_shield_error")) {
            return;
        }
        this.services.notification.add(noticeText(params.get("form_shield_error")), {
            title: _t("Not sent"),
            type: "danger",
            sticky: false,
        });
        params.delete("form_shield_error");
        const query = params.toString();
        const clean = document.location.pathname + (query ? "?" + query : "");
        window.history.replaceState({}, "", clean);
    }
}

registry.category("public.interactions").add("form_shield.FormShieldErrorNotice", FormShieldErrorNotice);
