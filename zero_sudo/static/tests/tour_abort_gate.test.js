/** @odoo-module **/
/* SPDX-License-Identifier: AGPL-3.0-or-later */

import { describe, expect, test } from "@odoo/hoot";
import { abortChannel, describePhase, describeRejectionReason } from "@zero_sudo/js/tour_abort_gate";

describe.tags("zero_sudo_tour_abort_gate");
describe("tour watchdog abort gate", () => {
    test("an Event rejection is named by type and target, other reasons pass through", () => {
        // Tests [@ANCHOR: zero_sudo:js_describe_rejection_reason]
        const link = document.createElement("link");
        link.href = "http://127.0.0.1/web/bundle/portal.assets_chatter_style";
        const withTarget = describeRejectionReason(
            Object.defineProperty(new Event("error"), "target", { value: link })
        );
        expect(withTarget).toInclude("[object Event]");
        expect(withTarget).toInclude("Event type=error");
        expect(withTarget).toInclude("target=LINK");
        expect(withTarget).toInclude("portal.assets_chatter_style");
        // A rejection event that has lost its target (a shadow-root link, a closing page).
        expect(describeRejectionReason(new Event("error"))).toInclude("target=none");
        const error = new Error("x");
        expect(describeRejectionReason(error)).toBe(error);
        expect(describeRejectionReason("text")).toBe("text");
    });

    test("describePhase says whether the tour succeeded and the page is unloading", () => {
        // Tests [@ANCHOR: zero_sudo:js_describe_phase]
        const doc = { readyState: "complete", visibilityState: "visible" };
        expect(describePhase({ succeededAt: 0, unloadingAt: 0 }, 1000, doc)).toBe(
            "Phase: tour succeeded no; unloading no; readyState=complete; visibility=visible"
        );
        expect(describePhase({ succeededAt: 900, unloadingAt: 950 }, 1000, doc)).toBe(
            "Phase: tour succeeded 100 ms ago; unloading 50 ms ago; readyState=complete; visibility=visible"
        );
    });

    test("an alarm is fatal until the tour has succeeded, a teardown notice after", () => {
        // Tests [@ANCHOR: zero_sudo:js_abort_channel]
        expect(abortChannel({ succeededAt: 0, unloadingAt: 0 })).toBe("fatal");
        // An unload during a step (expectUnloadPage) is part of the tour, not a teardown.
        expect(abortChannel({ succeededAt: 0, unloadingAt: 500 })).toBe("fatal");
        expect(abortChannel({ succeededAt: 900, unloadingAt: 0 })).toBe("teardown");
        expect(abortChannel({ succeededAt: 900, unloadingAt: 950 })).toBe("teardown");
        expect(abortChannel(undefined)).toBe("fatal");
    });
});
