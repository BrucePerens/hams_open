/** @odoo-module **/
/* SPDX-License-Identifier: AGPL-3.0-or-later
 * Part of Odoo. See LICENSE file for full copyright and licensing details.
 * This file is part of the HAMS project and is licensed under the AGPL-3.0-or-later license.
 * See the LICENSE file in the project root for full license information.
 */

// # Tests [@ANCHOR: UX_BINARY_INSTALL]
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

registry.category("web_tour.tours").add("binary_install_tour", {
    url: "/odoo?debug=1&action=binary_downloader.action_binary_downloader_manifest",
    steps: () => [
        TourUtils.waitForAbsence('.o_loading', 'Wait for initial load'),
        { trigger: 'body', content: 'Initialize Tour' },
        {
            trigger: '.o_list_button_add',
            run: 'click',
        },
        {
            trigger: 'div[name="name"] input',
            run: 'edit tourbin',
        },
        {
            trigger: 'div[name="url"] input',
            run: function (_helpers) {
                // Ensure focus for deterministicInput
                document.querySelector('div[name="url"] input').focus();
            }
        },
        {
            content: "Provide a valid downloadable URL pointing to the test controller",
            trigger: 'div[name="url"] input',
            run: function (helpers) {
                TourUtils.deterministicInput(helpers, 'https://dummy.example.com/test/dummy_bin');
            },
        },
        {
            trigger: 'div[name="checksum"] input',
            run: function (_helpers) {
                document.querySelector('div[name="checksum"] input').focus();
            }
        },
        {
            content: "Provide the exact SHA256 hash for the string '1234'",
            trigger: 'div[name="checksum"] input',
            run: 'edit 03ac674216f3e15c761ee1a5e255f067953623c8b388b4459e13f978d7c846f4',
        },
        {
            trigger: '.o_form_sheet',
            content: 'Click away to force DOM blur and commit text input',
            run: 'click',
        }
    ].concat(TourUtils.safeSave()).concat([
        // action_install()'s own display_notification client action
        // chains a "next": {"tag": "reload"} action right behind the
        // RPC's response. expectUnloadPage does NOT fit this: per Odoo's
        // own web_tour engine (tour_automatic.js/macro.js), that flag
        // makes the JS macro halt silently (no success AND no error
        // signal) expecting a real page navigation to load a NEW page
        // that resumes the tour from persisted tourState -- this
        // headless single-page test harness's own start_tour()/
        // browser_js() wrapper (zero_sudo/tests/common.py) has no such
        // resume-after-reload mechanism, so with the flag set the tour
        // just hangs for the full 10s timeout with zero further JS-side
        // log output, confirmed across two real runs (flag on the click
        // step alone, then on both the click and the notification-wait
        // step). ensure_executable() inside action_install() already
        // runs synchronously before the RPC responds -- the click's own
        // 200 OK response IS the real success signal -- so the tour
        // simply ends here; the actual installed-on-disk outcome is
        // asserted at the DB/filesystem level in the Python test instead
        // of chasing the notification's own reload race in JS.
        {
            content: "Click Install Now using immutable name attribute",
            trigger: 'button[name="action_install"]:not(:disabled)',
            run: 'click',
        },
    ]),
});
