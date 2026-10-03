# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The harness's navigator overrides must not break hoot's navigator mock.

hoot builds its mock with `createMock(navigator, ...)` (web/static/lib/hoot/hoot_utils.js). The harness used to
define `onLine` and `virtualKeyboard` on the `navigator` instance; once the instance had own keys, the mock
inherited from it and every other attribute read on the mock threw "TypeError: Illegal invocation" (seen on
mac1's VM as hoot suites aborting at `@barcodes/barcode_service`). This test runs in a real headless browser,
after both copies of the overrides (document start and browser_js's `ready` script) have run, and rebuilds the
mock with hoot's own algorithm. Against the old instance-level overrides it fails deterministically."""
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests.common import tagged

# hoot's createMock, copied from web/static/lib/hoot/hoot_utils.js (Odoo 19), with the one descriptor
# that matters here. Copied rather than imported: hoot's file is an Odoo module with bare import
# specifiers that only the /web/tests bundle can load.
_CHECK_JS = """
(() => {
    function createMock(target, descriptors) {
        let owner = target;
        let keys = Reflect.ownKeys(owner);
        while (!keys.length) {
            owner = Object.getPrototypeOf(owner);
            keys = Reflect.ownKeys(owner);
        }
        const mock = Object.assign(Object.create(owner), target);
        for (const property of keys) {
            Object.defineProperty(mock, property, {
                get() { return target[property]; },
                set(value) { target[property] = value; },
                configurable: true,
            });
        }
        for (const [property, descriptor] of Object.entries(descriptors)) {
            Object.defineProperty(mock, property, descriptor);
        }
        return mock;
    }

    const ownKeys = Reflect.ownKeys(navigator);
    if (ownKeys.length) {
        throw new Error("navigator has own keys " + JSON.stringify(ownKeys.map(String)) +
            "; hoot's createMock would inherit from the instance");
    }
    if (navigator.onLine !== true) {
        throw new Error("navigator.onLine override missing: " + navigator.onLine);
    }
    if (!("virtualKeyboard" in navigator) || typeof navigator.virtualKeyboard.addEventListener !== "function") {
        throw new Error("navigator.virtualKeyboard stub missing");
    }
    if (window.__hamsNavigatorOverridesVia !== "new-document") {
        throw new Error("navigator overrides were not installed at document start: " +
            window.__hamsNavigatorOverridesVia);
    }

    // Reads that threw "Illegal invocation" with the instance-level overrides. `platform` is
    // deliberately left undescribed here: an unpatched hoot does not describe it either.
    const mock = createMock(navigator, { userAgent: { get: () => "hams-test" } });
    const seen = {
        platform: mock.platform,
        language: mock.language,
        hardwareConcurrency: mock.hardwareConcurrency,
        onLine: mock.onLine,
        virtualKeyboard: typeof mock.virtualKeyboard.addEventListener,
        userAgent: mock.userAgent,
    };
    if (seen.onLine !== true || seen.virtualKeyboard !== "function" || seen.userAgent !== "hams-test") {
        throw new Error("hoot-style mock lost the overrides: " + JSON.stringify(seen));
    }
    console.log("test successful");
})();
"""


@tagged("post_install", "-at_install")
class TestNavigatorOverrides(HamsHttpCase):
    # Tests [@ANCHOR: zero_sudo:install_navigator_overrides]
    # Tests [@ANCHOR: zero_sudo:hams_http_case_browser_js]
    def test_hoot_navigator_mock_survives_the_harness_overrides(self):
        self.browser_js("/web/login", _CHECK_JS, "", timeout=60)
