/** @odoo-module **/
/* SPDX-License-Identifier: AGPL-3.0-or-later */

import { registry } from "@web/core/registry";

// A tour whose page misbehaves the moment the tour has reported success: it waits for the
// watchdog's own record of that (window._hamsPhase.succeededAt, set when "tour succeeded" is
// logged) and then rejects a promise with an Event, as Odoo's lazily loaded chatter stylesheet
// does when its load is cut off. The Python test
// (tests/test_tour_abort_gate.py) requires the run to pass and the notices to be warnings.
registry.category("web_tour.tours").add("zero_sudo_tour_abort_gate", {
    url: "/web/login",
    steps: () => [
        {
            content: "Arm the late failures",
            trigger: "body",
            run() {
                const poll = setInterval(() => {
                    if (window._hamsPhase && window._hamsPhase.succeededAt) {
                        clearInterval(poll);
                        Promise.reject(new Event("error"));
                    }
                }, 2);
                setTimeout(() => clearInterval(poll), 10000);
            },
        },
    ],
});
